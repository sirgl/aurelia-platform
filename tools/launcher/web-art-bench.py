#!/usr/bin/env python3
"""Measures how fast the browser build shows its pictures (lands/docs/web.md, "Picture loading").

  python3 -I platform/tools/launcher/web-art-bench.py URL [--runs 3] [--warm] [--jiggle 0|1] [--json OUT.json]

URL is the page with `?trace=1` (added when missing). One run, in a fresh Chrome. Default: headed, real GPU (wrap the call in
`flock .../gpu.lock`; a window behind others must keep drawing, see the flags). With `BENCH_HEADLESS=1`
and `--gpu swiftshader` and a `?backend=gl` URL it runs headless on the software GL backend: no GPU lock,
slower frames, the picture pipeline is the same (this is what the numbers in lands/docs/web.md come from): load the page, wait for the menu cover, click "New game", wait until
the screen's pictures are shown, click "Start game", wait for the map and its HUD art.
`--warm` loads the page once first, so the HTTP cache is warm for the measured load.
The mouse is moved every 100 ms (`--jiggle 1`, default): a player's hand does the same, and
a page that draws only on events needs it; `--jiggle 0` leaves the mouse alone.
Click coordinates are CSS pixels of a 1416x732 page (the window is 1416x875 headless) (the screen layout is in points, so they only hold for that window size).
Prints per run: ms since navigation of the menu frame, the cover shown, the first and last
picture of the new-game screen, the map, and the main-thread long tasks (> 50 ms) per phase.
"""
import importlib.util, json, os, subprocess, sys, tempfile, time, urllib.request

spec = importlib.util.spec_from_file_location("webcdp", os.path.join(os.path.dirname(os.path.abspath(__file__)), "web-cdp.py"))
cdp = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cdp)

NEW_GAME = (251, 357)
START_GAME = (1108, 657)
READ = "JSON.stringify({now: performance.now(), a: window.__aurelia || null, t: window.__trace || [], l: window.__long || [], res: performance.getEntriesByType('resource').map(e => [e.name.split('/assets/')[1] || e.name, e.startTime, e.responseEnd, e.transferSize, e.decodedBodySize])})"


class Run:
    def __init__(self, c):
        self.c = c
        self.mouse = 0

    def ev(self, js):
        r = self.c.send("Runtime.evaluate", expression=js, awaitPromise=True, returnByValue=True)["result"]["result"]
        return r.get("value")

    def state(self):
        return json.loads(self.ev(READ))

    def jiggle(self, on):
        if on:
            self.mouse += 1
            self.c.send("Input.dispatchMouseEvent", type="mouseMoved", x=700 + (self.mouse % 2) * 20, y=300)
        self.c.pump(0.1)

    def click(self, xy):
        for t, extra in (("mouseMoved", {}), ("mouseMoved", {}), ("mousePressed", {"button": "left", "clickCount": 1}), ("mouseReleased", {"button": "left", "clickCount": 1})):
            self.c.send("Input.dispatchMouseEvent", type=t, x=xy[0] + (1 if t == "mouseMoved" and extra == {} and self.mouse % 2 else 0), y=xy[1], **extra)
            self.mouse += 1
            self.c.pump(0.25 if t == "mouseMoved" else 0.05)


def _click_until(self, xy, done, jiggle):
    """Clicks, and again every 3 s while the click had no effect (a frame may be slow)."""
    for _ in range(8):
        self.click(xy)
        t_end = time.time() + 3
        while time.time() < t_end:
            self.jiggle(jiggle)
            a = self.state()["a"]
            if a and done(a):
                return True
    return False


Run.click_until = _click_until


def first(rows, kind, name=None):
    for r in rows:
        if r[1] == kind and (name is None or r[2] == name):
            return r[0]
    return None


def measure(c, url, jiggle, limit=60):
    run = Run(c)
    out = {}
    c.send("Page.navigate", url=url)
    t_end = time.time() + 40
    while time.time() < t_end:
        c.pump(0.1)
        try:
            s = run.state()
        except Exception:
            continue
        if s["a"] and s["a"]["frames"] > 0:
            out["menu_frame_ms"] = s["now"]
            break
    t_end = time.time() + limit
    while time.time() < t_end:
        run.jiggle(jiggle)
        s = run.state()
        if first(s["t"], "shown", "menu") is not None:
            break
    out["cover_shown_ms"] = first(s["t"], "shown", "menu")
    # New game
    s = run.state()
    t_click = s["now"]
    run.click_until(NEW_GAME, lambda a: a["screen"] != "Menu", jiggle)
    out["click_new_game_ms"] = t_click
    quiet_since, last_count = time.time(), 0
    t_end = time.time() + limit
    while time.time() < t_end:
        run.jiggle(jiggle)
        s = run.state()
        shown = len([r for r in s["t"] if r[1] == "shown" and r[0] >= t_click])
        if shown != last_count:
            last_count, quiet_since = shown, time.time()
        if s["a"] and s["a"]["screen"] != "Menu" and time.time() - quiet_since > 4:
            break
    ng = [r for r in s["t"] if r[0] >= t_click]
    asked = [r for r in ng if r[1] == "ask"]
    shown_rows = [r for r in ng if r[1] == "shown"]
    out["ng_asked"] = len(asked)
    out["ng_shown"] = len(shown_rows)
    out["ng_first_shown_after_click_ms"] = min((r[0] for r in shown_rows), default=None)
    out["ng_last_shown_ms"] = max((r[0] for r in shown_rows), default=None)
    out["ng_shown_times"] = sorted(round(r[0]) for r in shown_rows)
    out["screen"] = s["a"]["screen"] if s["a"] else None
    t_start = s["now"]
    run.click_until(START_GAME, lambda a: a["screen"] == "Map", jiggle)
    out["click_start_ms"] = t_start
    t_end = time.time() + limit
    quiet_since, last_count = time.time(), 0
    while time.time() < t_end:
        run.jiggle(jiggle)
        s = run.state()
        shown = len([r for r in s["t"] if r[1] == "shown" and r[0] >= t_start])
        if shown != last_count:
            last_count, quiet_since = shown, time.time()
        if s["a"] and s["a"]["screen"] == "Map" and "map_ms" not in out:
            out["map_ms"] = s["now"]
        if "map_ms" in out and time.time() - quiet_since > 4:
            break
    out["map_screen"] = s["a"]["screen"] if s["a"] else None
    map_rows = [r for r in s["t"] if r[0] >= t_start and r[1] == "shown"]
    out["map_shown"] = len(map_rows)
    out["map_last_shown_ms"] = max((r[0] for r in map_rows), default=None)
    out["end_ms"] = s["now"]
    # long tasks per phase
    phases = {"boot": (0, out.get("cover_shown_ms") or out["click_new_game_ms"]), "menu": (0, out["click_new_game_ms"]),
              "newgame": (out["click_new_game_ms"], out["click_start_ms"]), "map": (out["click_start_ms"], s["now"])}
    for name, (a, b) in phases.items():
        tasks = [d for (st, d) in s["l"] if a <= st < b]
        out[f"long_{name}"] = [len(tasks), round(sum(tasks)), round(max(tasks, default=0))]
    fetches = [r for r in s["res"] if r[0].startswith("visual") or r[0].startswith("fonts") or r[0].endswith(".tsv")]
    out["fetched_files"] = len(fetches)
    out["fetched_bytes"] = sum(r[3] for r in fetches)
    out["decoded_bytes"] = sum(r[4] for r in fetches)
    out["trace"] = s["t"]
    out["resources"] = s["res"]
    decodes = [r for r in s["t"] if r[1] == "decode"]
    out["decodes"] = len(decodes)
    return out


def main():
    args = sys.argv[1:]
    url = args[0]
    runs, warm, jiggle, json_out, gpu = 3, False, True, None, "real"
    i = 1
    while i < len(args):
        if args[i] == "--runs": runs = int(args[i + 1]); i += 2
        elif args[i] == "--warm": warm = True; i += 1
        elif args[i] == "--jiggle": jiggle = args[i + 1] != "0"; i += 2
        elif args[i] == "--json": json_out = args[i + 1]; i += 2
        elif args[i] == "--gpu": gpu = args[i + 1]; i += 2
        else: i += 1
    if "trace=1" not in url:
        url += ("&" if "?" in url else "?") + "trace=1"
    results = []
    for n in range(runs):
        prof = tempfile.mkdtemp(prefix="bench-prof-")
        headless = ["--headless=new"] if os.environ.get("BENCH_HEADLESS") else []
        flags = [*headless, "--no-sandbox", f"--user-data-dir={prof}", "--remote-debugging-port=0", "--enable-unsafe-webgpu", "--ignore-gpu-blocklist",
                 "--window-size=1416,875", "--no-first-run", "--force-device-scale-factor=1",
                 # a window behind others must keep drawing (the compositor sends no frame callbacks to it otherwise)
                 "--disable-backgrounding-occluded-windows", "--disable-renderer-backgrounding", "--disable-background-timer-throttling",
                 "--disable-features=CalculateNativeWinOcclusion"]
        if gpu == "real":
            flags += ["--enable-features=Vulkan,WebGPU", "--use-angle=vulkan", "--disable-vulkan-surface"]
        else:  # software rendering (headed): no GPU lock needed, the frames are slower
            flags += ["--enable-unsafe-swiftshader", "--use-webgpu-adapter=swiftshader", "--use-angle=swiftshader"]
        chrome = subprocess.Popen(["google-chrome", *flags, "about:blank"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            port_file = os.path.join(prof, "DevToolsActivePort")
            for _ in range(100):
                if os.path.exists(port_file): break
                time.sleep(0.2)
            port = open(port_file).read().split()[0]
            pages = json.load(urllib.request.urlopen(f"http://127.0.0.1:{port}/json/list"))
            page = [p for p in pages if p["type"] == "page"][0]
            c = cdp.Cdp(page["webSocketDebuggerUrl"])
            for d in ("Page", "Runtime", "Log"): c.send(f"{d}.enable")
            if warm:
                c.send("Page.navigate", url=url)
                c.pump(25)
            load = os.getloadavg()[0]
            r = measure(c, url, jiggle)
            r["load"] = load
            r["inner"] = c.send("Runtime.evaluate", expression="innerWidth+'x'+innerHeight+'@'+devicePixelRatio", returnByValue=True)["result"]["result"]["value"]
            r["console"] = []
            for e in c.events:
                if e["method"] == "Runtime.exceptionThrown":
                    r["console"].append("EXCEPTION " + json.dumps(e["params"]["exceptionDetails"])[:500])
                elif e["method"] == "Runtime.consoleAPICalled" and e["params"]["type"] in ("error", "warning"):
                    r["console"].append(" ".join(str(a.get("value", a.get("description", ""))) for a in e["params"]["args"])[:300])
            results.append(r)
            for line in r["console"][:10]: print("  console:", line)
            print(f"run {n + 1}/{runs} load {load:.1f}", {k: v for k, v in r.items() if k not in ("trace", "resources", "ng_shown_times", "console")}, flush=True)
        finally:
            chrome.terminate()
            try: chrome.wait(10)
            except Exception: chrome.kill()
    if json_out:
        json.dump(results, open(json_out, "w"))


main()
