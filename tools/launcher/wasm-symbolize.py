#!/usr/bin/env python3
"""Names for the `wasm-function[N]` frames of a panic trace of the browser build (classic/docs/web-traces.md).

  python3 -I platform/tools/launcher/wasm-symbolize.py symbols/web_bg.<hash>.wasm  < trace.txt
  python3 -I platform/tools/launcher/wasm-symbolize.py symbols/web_bg.<hash>.wasm  trace.txt
  python3 -I platform/tools/launcher/wasm-symbolize.py symbols/web_bg.<hash>.wasm  -f 41873 40211   (function indices)

The symbols file is the same wasm as the page's with its `name` section kept (web-build.sh writes it next to
dist/, never into it). Every `wasm-function[N]` in the input gets the function's name in front of it."""
import re, sys


def leb(b, i):
    r = s = 0
    while True:
        x = b[i]; i += 1; r |= (x & 0x7F) << s; s += 7
        if not x & 0x80:
            return r, i


def function_names(path):
    b = open(path, "rb").read()
    i = 8
    while i < len(b):
        sid = b[i]; size, j = leb(b, i + 1); end = j + size
        if sid == 0:
            n, k = leb(b, j)
            if b[k:k + n] == b"name":
                k += n
                while k < end:
                    sub = b[k]; ssize, k = leb(b, k + 1); send = k + ssize
                    if sub == 1:  # function names: vec of (index, name)
                        count, k = leb(b, k)
                        names = {}
                        for _ in range(count):
                            idx, k = leb(b, k); ln, k = leb(b, k)
                            names[idx] = b[k:k + ln].decode("utf-8", "replace"); k += ln
                        return names
                    k = send
        i = end
    sys.exit(f"{path}: no function names (this is the shipped wasm; use the file in symbols/)")


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    names = function_names(sys.argv[1])
    if len(sys.argv) > 2 and sys.argv[2] == "-f":
        for n in sys.argv[3:]:
            print(f"wasm-function[{n}]  {names.get(int(n), '?')}")
        return
    text = open(sys.argv[2]).read() if len(sys.argv) > 2 else sys.stdin.read()
    sys.stdout.write(re.sub(r"wasm-function\[(\d+)\]", lambda m: f"{names.get(int(m.group(1)), '?')} ({m.group(0)})", text))


main()
