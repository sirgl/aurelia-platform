#!/usr/bin/env python3
"""A static server for the browser build that behaves like a CDN would (lands/docs/web.md).

  python3 platform/tools/launcher/web-serve.py [DIR=dist/web] [--port 8080] [--cache headers|revalidate|immutable|none]
                              [--bind 127.0.0.1] [--delay-ms N] [--kbps N]

`python3 -m http.server` sends no ETag, no Cache-Control and no compression, so a reload
fetches 60 MB of pictures again and the 60 MB wasm goes over the wire raw. This one:
  * ETag (size + mtime) and `304 Not Modified` on If-None-Match / If-Modified-Since;
  * the headers of DIR/_headers (Cloudflare Pages syntax: a path pattern with `*` / `:name` placeholders, its
    headers indented below; all matching rules apply, equal names are joined with ", "), exactly what the
    CDN will send: `--cache headers`, the default when DIR has a _headers file. Else Cache-Control by mode:
    `revalidate` (always asks, 304 costs no body), `immutable` (one year) or `none` (no header at all);
  * gzip for .wasm .js .html .tsv .json .txt (compressed once, kept in memory) when the browser accepts it;
    images are already compressed and go as they are;
  * `--delay-ms` adds latency to every response and `--kbps` caps the rate (a poor man's network, to
    see what a real link would do; 0 = off);
  * Content-Type image/webp, application/wasm; CORS `*` (the page may take its art from another origin).
Serves each connection in a thread (a browser opens 6 at once). Stop it by its PID.
"""
import argparse, gzip, http.server, mimetypes, os, re, socketserver, sys, time, email.utils

COMPRESS = {".wasm", ".js", ".html", ".tsv", ".json", ".txt", ".css", ".svg", ".webmanifest"}
TYPES = {".gz": "application/octet-stream", ".bin": "application/octet-stream", ".wasm": "application/wasm", ".webp": "image/webp", ".js": "text/javascript", ".tsv": "text/tab-separated-values", ".ttf": "font/ttf", ".otf": "font/otf"}
GZIPPED = {}


RULES = []  # [(compiled path pattern, [(header, value)])] from _headers


def load_headers(path):
    """Cloudflare Pages `_headers`: unindented lines are path patterns, indented `Name: value` lines belong to the last one."""
    rules = []
    for line in open(path, encoding="utf-8"):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        if line[0] not in " \t":
            parts = re.split(r"(\*|:[A-Za-z]\w*)", line.strip())
            pattern = "".join("(.*)" if p == "*" else "([^/]+)" if p.startswith(":") else re.escape(p) for p in parts)
            rules.append((re.compile("^" + pattern + "$"), []))
        elif rules and ":" in line:
            name, value = line.split(":", 1)
            rules[-1][1].append((name.strip(), value.strip()))
    return rules


def headers_for(url_path):
    joined = {}
    for pattern, headers in RULES:
        if pattern.match(url_path):
            for name, value in headers:
                joined[name] = joined[name] + ", " + value if name in joined else value
    return joined


class Handler(http.server.SimpleHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    cache = "revalidate"
    delay = 0.0
    kbps = 0

    def log_message(self, fmt, *args):
        if os.environ.get("WEB_SERVE_LOG"):
            sys.stderr.write("%s %s\n" % (self.address_string(), fmt % args))

    def send_head_body(self, head_only):
        path = self.translate_path(self.path.split("?", 1)[0])
        if os.path.isdir(path):
            path = os.path.join(path, "index.html")
        if not os.path.isfile(path):
            self.send_error(404, "File not found")
            return
        st = os.stat(path)
        ext = os.path.splitext(path)[1].lower()
        etag = '"%x-%x"' % (st.st_size, int(st.st_mtime_ns // 1000))
        if self.delay:
            time.sleep(self.delay)
        if self.headers.get("If-None-Match") == etag:
            self.send_response(304)
            self.send_header("ETag", etag)
            self.cache_headers(path)
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        with open(path, "rb") as f:
            body = f.read()
        encoding = None
        if ext in COMPRESS and "gzip" in self.headers.get("Accept-Encoding", ""):
            key = (path, etag)
            if key not in GZIPPED:
                GZIPPED[key] = gzip.compress(body, 6)
            body, encoding = GZIPPED[key], "gzip"
        self.send_response(200)
        self.send_header("Content-Type", TYPES.get(ext) or mimetypes.guess_type(path)[0] or "application/octet-stream")
        self.send_header("Content-Length", str(len(body)))
        if encoding:
            self.send_header("Content-Encoding", encoding)
        self.send_header("Vary", "Accept-Encoding")
        self.send_header("ETag", etag)
        self.send_header("Last-Modified", email.utils.formatdate(st.st_mtime, usegmt=True))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.cache_headers(path)
        self.end_headers()
        if head_only:
            return
        if self.kbps:
            chunk = max(1024, self.kbps * 1000 // 8 // 50)
            for i in range(0, len(body), chunk):
                self.wfile.write(body[i:i + chunk])
                time.sleep(chunk * 8 / (self.kbps * 1000))
        else:
            self.wfile.write(body)

    def cache_headers(self, path):
        if self.cache == "headers":
            for name, value in headers_for(self.path.split("?", 1)[0]).items():
                self.send_header(name, value)
        elif self.cache == "immutable":
            self.send_header("Cache-Control", "public, max-age=31536000, immutable")
        elif self.cache == "revalidate":
            self.send_header("Cache-Control", "no-cache")

    def do_GET(self):
        self.send_head_body(False)

    def do_HEAD(self):
        self.send_head_body(True)


class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    allow_reuse_address = True
    request_queue_size = 64


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir", nargs="?", default="dist/web")
    ap.add_argument("--port", type=int, default=8080)
    ap.add_argument("--bind", default="127.0.0.1")
    ap.add_argument("--cache", choices=["headers", "revalidate", "immutable", "none"], default=None)
    ap.add_argument("--delay-ms", type=float, default=0)
    ap.add_argument("--kbps", type=int, default=0)
    args = ap.parse_args()
    os.chdir(args.dir)
    if args.cache is None:
        args.cache = "headers" if os.path.isfile("_headers") else "revalidate"
    if args.cache == "headers":
        RULES.extend(load_headers("_headers"))
    Handler.cache, Handler.delay, Handler.kbps = args.cache, args.delay_ms / 1000, args.kbps
    server = Server((args.bind, args.port), Handler)
    print(f"serving {os.getcwd()} on http://{args.bind}:{args.port}/ (cache: {args.cache}, pid {os.getpid()})", flush=True)
    server.serve_forever()


main()
