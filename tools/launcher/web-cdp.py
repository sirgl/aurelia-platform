#!/usr/bin/env python3
"""A tiny Chrome DevTools driver (stdlib only) to check the browser build by eye and by number.

  python3 -I platform/tools/launcher/web-cdp.py URL OUT_DIR [--gpu real|swiftshader] [--wait SECONDS]
         [--size 1440x900] [--eval JS]... [--click X,Y]... [--shot NAME]...
         [--phone] [--dpr 3] [--pre METHOD JSON]... [--cdp METHOD JSON]... [--tap X,Y]... [--hold X,Y,S]...
         [--swipe X1,Y1,X2,Y2]... [--pinch X,Y,FROM,TO]... [--type TEXT]...

--phone emulates a phone (touch events, mobile viewport, the Android Chrome user agent, device pixel
ratio --dpr); --pre sends a DevTools command before the page loads (network throttling:
`--pre Network.emulateNetworkConditions '{"offline":false,"latency":40,"downloadThroughput":625000,"uploadThroughput":125000}'`),
--cdp sends one at that point of the steps. --tap/--hold/--swipe/--pinch send touch events.

Starts a headless Chrome with WebGPU enabled, opens URL, collects console output and page
errors, waits, then runs the steps in the order given: --eval prints the JSON of the
expression, --click sends a mouse click, --shot saves OUT_DIR/NAME.png. Chrome is stopped by
its PID at the end. Real GPU runs must be wrapped in `flock .../gpu.lock` by the caller.
"""
import base64, json, os, socket, struct, subprocess, sys, tempfile, time, urllib.request

def ws_connect(url):
    host, port, path = url[5:].split("/", 1)[0].split(":")[0], int(url[5:].split("/", 1)[0].split(":")[1]), "/" + url[5:].split("/", 1)[1]
    s = socket.create_connection((host, port))
    key = base64.b64encode(os.urandom(16)).decode()
    s.sendall((f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").encode())
    buf = b""
    while b"\r\n\r\n" not in buf:
        buf += s.recv(4096)
    return s, buf.split(b"\r\n\r\n", 1)[1]

class Cdp:
    def __init__(self, url):
        self.s, self.buf = ws_connect(url)
        self.id = 0
        self.events = []
    def _read(self, n):
        while len(self.buf) < n:
            chunk = self.s.recv(1 << 20)
            if not chunk:
                raise EOFError
            self.buf += chunk
        out, self.buf = self.buf[:n], self.buf[n:]
        return out
    def _frame(self):
        data = b""
        while True:
            b0, b1 = self._read(2)
            n = b1 & 0x7F
            if n == 126: n = struct.unpack(">H", self._read(2))[0]
            elif n == 127: n = struct.unpack(">Q", self._read(8))[0]
            payload = self._read(n)
            data += payload
            if b0 & 0x80:
                return data
    def send(self, method, **params):
        self.id += 1
        msg = json.dumps({"id": self.id, "method": method, "params": params}).encode()
        mask = os.urandom(4)
        n = len(msg)
        head = b"\x81" + (bytes([0x80 | n]) if n < 126 else b"\xfe" + struct.pack(">H", n) if n < 65536 else b"\xff" + struct.pack(">Q", n))
        self.s.sendall(head + mask + bytes(c ^ mask[i % 4] for i, c in enumerate(msg)))
        want = self.id
        while True:
            m = json.loads(self._frame())
            if m.get("id") == want:
                return m
            if "method" in m:
                self.events.append(m)
    def pump(self, seconds):
        end = time.time() + seconds
        self.s.settimeout(0.5)
        while time.time() < end:
            try:
                m = json.loads(self._frame())
                if "method" in m:
                    self.events.append(m)
            except (socket.timeout, TimeoutError):
                pass
        self.s.settimeout(None)

GRAB = """(() => {
  window.__shoot = () => new Promise((resolve) => {
    // Two frames on, the canvas shows what the page last presented.
    requestAnimationFrame(() => requestAnimationFrame(() => {
      const canvas = document.querySelector("canvas");
      const copy = document.createElement("canvas");
      copy.width = canvas.width; copy.height = canvas.height;
      copy.getContext("2d").drawImage(canvas, 0, 0);
      resolve(copy.toDataURL("image/png"));
    }));
  });
})();"""

def main():
    args = sys.argv[1:]
    url, out = args[0], args[1]
    gpu, wait, size, steps, pre, phone, dpr = "swiftshader", 20, "1440x900", [], [], False, 1.0
    i = 2
    while i < len(args):
        a = args[i]
        if a == "--gpu": gpu = args[i + 1]
        elif a == "--wait": wait = float(args[i + 1])
        elif a == "--size": size = args[i + 1]
        elif a == "--dpr": dpr = float(args[i + 1])
        elif a == "--phone":
            phone = True
            i += 1
            continue
        elif a == "--pre":
            pre.append((args[i + 1], json.loads(args[i + 2])))
            i += 3
            continue
        elif a == "--cdp":
            steps.append(("--cdp", (args[i + 1], json.loads(args[i + 2]))))
            i += 3
            continue
        elif a == "--script":
            # one step per line: `eval JS`, `click X,Y`, `move X,Y`, `sleep S`, `shot NAME`, `grab NAME`
            for line in open(args[i + 1]):
                if line.strip() and not line.startswith("#"):
                    kind, _, rest = line.rstrip("\n").partition(" ")
                    steps.append(("--" + kind, rest))
        elif a in ("--eval", "--click", "--shot", "--sleep", "--grab", "--move", "--drag", "--wheel", "--tap", "--hold", "--swipe", "--pinch", "--type"): steps.append((a, args[i + 1]))
        i += 2
    os.makedirs(out, exist_ok=True)
    prof = tempfile.mkdtemp(prefix="cdp-prof-")
    headless = [] if os.environ.get("CDP_HEADED") else ["--headless=new"]
    flags = [*headless, "--no-sandbox", f"--user-data-dir={prof}", "--remote-debugging-port=0", "--enable-unsafe-webgpu",
             "--ignore-gpu-blocklist", f"--window-size={size.replace('x', ',')}", "--no-first-run"]
    if gpu == "bare":
        flags += ["--enable-unsafe-swiftshader"]
    elif gpu == "swiftshader":
        flags += ["--enable-unsafe-swiftshader", "--use-webgpu-adapter=swiftshader", "--use-angle=swiftshader"]
    else:
        flags += ["--enable-features=Vulkan,WebGPU", "--use-angle=vulkan", "--disable-vulkan-surface"]
    flags += os.environ.get("CDP_FLAGS", "").split()
    if os.environ.get("CDP_NO_WEBGPU"):  # the WebGL2 path: a browser without WebGPU
        flags = [f for f in flags if "webgpu" not in f.lower() and f != "--enable-features=Vulkan,WebGPU"]
    chrome = subprocess.Popen(["google-chrome", *flags, "about:blank"], stdout=subprocess.DEVNULL, stderr=open(os.environ["CDP_LOG"], "w") if os.environ.get("CDP_LOG") else subprocess.DEVNULL)
    try:
        port_file = os.path.join(prof, "DevToolsActivePort")
        for _ in range(100):
            if os.path.exists(port_file): break
            time.sleep(0.2)
        port = open(port_file).read().split()[0]
        pages = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list"))
        page = [p for p in pages if p["type"] == "page"][0]
        c = Cdp(page["webSocketDebuggerUrl"])
        for d in ("Page", "Runtime", "Log"): c.send(f"{d}.enable")
        w, h = map(int, size.split("x"))
        if not os.environ.get("CDP_HEADED"):
            c.send("Emulation.setDeviceMetricsOverride", width=w, height=h, deviceScaleFactor=dpr, mobile=phone)
        if phone:
            c.send("Emulation.setTouchEmulationEnabled", enabled=True, maxTouchPoints=5)
            c.send("Emulation.setUserAgentOverride", userAgent="Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Mobile Safari/537.36",
                   userAgentMetadata={"brands": [{"brand": "Chromium", "version": "130"}], "platform": "Android", "platformVersion": "14", "architecture": "", "model": "Pixel 8", "mobile": True})
        for method, params in pre:
            c.send(method, **params)
        # A WebGPU canvas cannot be captured by the compositor in a headless software setup, so
        # the page copies it into a 2D canvas right after a queue submit (same task: still valid).
        c.send("Page.addScriptToEvaluateOnNewDocument", source=GRAB)
        c.send("Page.navigate", url=url)
        c.pump(wait)
        for kind, arg in steps:
            if kind == "--eval":
                r = c.send("Runtime.evaluate", expression=arg, awaitPromise=True, returnByValue=True)
                print("EVAL", json.dumps(r.get("result", r).get("result", r), ensure_ascii=False)[:2000])
            elif kind == "--cdp":
                r = c.send(arg[0], **arg[1])
                print("CDP", arg[0], json.dumps(r.get("result", r))[:300])
            elif kind == "--tap":
                x, y = map(float, arg.split(","))
                c.send("Input.dispatchTouchEvent", type="touchStart", touchPoints=[{"x": x, "y": y, "id": 1}])
                c.pump(0.06)
                c.send("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                c.pump(1.2)
            elif kind == "--hold":
                x, y, secs = map(float, arg.split(","))
                c.send("Input.dispatchTouchEvent", type="touchStart", touchPoints=[{"x": x, "y": y, "id": 1}])
                c.pump(secs)
                c.send("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                c.pump(1.0)
            elif kind == "--swipe":
                x1, y1, x2, y2 = map(float, arg.split(","))
                c.send("Input.dispatchTouchEvent", type="touchStart", touchPoints=[{"x": x1, "y": y1, "id": 1}])
                for k in range(1, 13):
                    c.send("Input.dispatchTouchEvent", type="touchMove", touchPoints=[{"x": x1 + (x2 - x1) * k / 12, "y": y1 + (y2 - y1) * k / 12, "id": 1}])
                    c.pump(0.03)
                c.send("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                c.pump(1.0)
            elif kind == "--pinch":
                x, y, d0, d1 = map(float, arg.split(","))
                pts = lambda d: [{"x": x - d / 2, "y": y, "id": 1}, {"x": x + d / 2, "y": y, "id": 2}]
                c.send("Input.dispatchTouchEvent", type="touchStart", touchPoints=pts(d0))
                for k in range(1, 13):
                    c.send("Input.dispatchTouchEvent", type="touchMove", touchPoints=pts(d0 + (d1 - d0) * k / 12))
                    c.pump(0.03)
                c.send("Input.dispatchTouchEvent", type="touchEnd", touchPoints=[])
                c.pump(1.0)
            elif kind == "--type":
                c.send("Input.insertText", text=arg)
                c.pump(0.8)
            elif kind == "--sleep":
                c.pump(float(arg))
            elif kind == "--click":
                x, y = map(float, arg.split(","))
                for t in ("mouseMoved", "mousePressed", "mouseReleased"):
                    c.send("Input.dispatchMouseEvent", type=t, x=x, y=y, button="left", clickCount=1)
                    c.pump(0.1)
                c.pump(1.5)
            elif kind == "--drag":
                x1, y1, x2, y2 = map(float, arg.split(","))
                c.send("Input.dispatchMouseEvent", type="mouseMoved", x=x1, y=y1)
                c.send("Input.dispatchMouseEvent", type="mousePressed", x=x1, y=y1, button="left", buttons=1, clickCount=1)
                for k in range(1, 11):
                    c.send("Input.dispatchMouseEvent", type="mouseMoved", x=x1 + (x2 - x1) * k / 10, y=y1 + (y2 - y1) * k / 10, button="left", buttons=1)
                    c.pump(0.05)
                c.send("Input.dispatchMouseEvent", type="mouseReleased", x=x2, y=y2, button="left", clickCount=1)
                c.pump(1)
            elif kind == "--wheel":
                x, y, dy = map(float, arg.split(","))
                c.send("Input.dispatchMouseEvent", type="mouseWheel", x=x, y=y, deltaX=0, deltaY=dy)
                c.pump(1)
            elif kind == "--move":
                x, y = map(float, arg.split(","))
                c.send("Input.dispatchMouseEvent", type="mouseMoved", x=x, y=y)
                c.pump(0.3)
            elif kind == "--grab":
                r = c.send("Runtime.evaluate", expression="window.__shoot()", awaitPromise=True, returnByValue=True)["result"]["result"].get("value", "")
                if r.startswith("data:image/png;base64,"):
                    open(os.path.join(out, arg + ".png"), "wb").write(base64.b64decode(r.split(",", 1)[1]))
                    print("GRAB", arg)
                else:
                    print("GRAB failed", r[:200])
            elif kind == "--shot":
                r = c.send("Page.captureScreenshot", format="png")
                open(os.path.join(out, arg + ".png"), "wb").write(base64.b64decode(r["result"]["data"]))
                print("SHOT", arg)
        c.pump(0.5)
        for e in c.events:
            if e["method"] == "Runtime.consoleAPICalled":
                text = " ".join(str(a.get("value", a.get("description", ""))) for a in e["params"]["args"])
                print("CONSOLE", e["params"]["type"], text[:400])
            elif e["method"] == "Runtime.exceptionThrown":
                print("EXCEPTION", json.dumps(e["params"]["exceptionDetails"])[:600])
            elif e["method"] == "Log.entryAdded":
                en = e["params"]["entry"]
                print("LOG", en["level"], en["text"][:300], en.get("url", ""))
    finally:
        chrome.terminate()
        try: chrome.wait(10)
        except Exception: chrome.kill()

if __name__ == "__main__":
    main()
