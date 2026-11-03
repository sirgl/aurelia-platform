# platform

The rules-independent part of Aurelia, a turn-based strategy game (an Unciv / Civ 5 inspired game with a
similar rule set, written in Rust). It is the engine that the game modules plug into; it knows no module.

| Crate | What it is |
|---|---|
| `kernel` | the deterministic rules kernel: world state, commands, journal, replay, saves, the AI contract, the generic map generator; generic over `RulesModule` |
| `render` | the wgpu renderer: terrain mesher, world models, units, fog, effects; shaders and baked models come from `assets/` |
| `client` | the app shell: windows, HUD kit, screens contract (`ModuleUi`), saves, art loader, input, the AI driver |
| `i18n` | localization: message catalogue, plural rules, the bundles of every part present are compiled in |
| `net` | multiplayer protocol and match logic (no I/O) |
| `sandbox-rules`, `sandbox-content`, `sandbox-ui` | a tiny reference module: the platform's own test game and the template for a new module |
| `ui-input` | the small glue between a winit window and egui (window events to egui input, egui output to the cursor, clipboard, links, IME); replaces `egui-winit`, which does not build for wasm32; MIT OR Apache-2.0, publishable on its own ([docs/ui-input.md](docs/ui-input.md)) |
| `launcher-core` | everything a game launcher needs that is not a rule set: the desktop window and frame loop, music, the browser page (`web_exports!`), the Android activity, the `--replay` report. A launcher registers ONE module with it |
| `prof` | a frame/section profiler |

## Build

```sh
rustup toolchain install 1.98.1
cd platform
cargo +1.98.1 check --workspace --all-targets      # the platform alone: no other folder is needed
cargo +1.98.1 test --release --workspace            # tests that open a GPU skip when there is no adapter
```

System libraries on Ubuntu / Debian: `build-essential pkg-config cmake clang libssl-dev libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev libvulkan-dev`; a Vulkan (Linux, Windows) or Metal (macOS) driver is needed to render.

## How it relates to the other parts

- **Aurelia: Classic** (repository `aurelia-classic`, <https://github.com/sirgl/aurelia-classic>), the game built on this engine, and
  **Lands & Laws** (repository `aurelia-lands`, <https://github.com/sirgl/aurelia-lands>) depend on this repository through
  path dependencies (`../../../platform/crates/...`): clone the repositories side by side into folders named `platform`, `classic` and `lands`.
  This folder depends on nothing outside itself; `sh tools/platform-lint.sh` counts every tie to a module and fails
  when one grows.
- Assets: shaders, fonts, the shared and generic 3D models (`assets/models/`: what both modules draw, the fallback,
  the city kit, the terrain props), audio and the shell's language bundles are here (`assets/`). A module owns
  its own rules data, art catalogs, language bundles and models (`<module>/assets/models/`); `crates/i18n`,
  `client::shell::assets` and the render build find the folders of every part that is present beside this one
 .
- `tools/launcher/`: packaging (`package.sh`), the web page build (`web-build.sh`), the Android build and screenshots, driven by each launcher's `launcher.conf`.
- `tools/`: lints (`platform-lint.sh`, `check-structure.sh`, `i18n-lint.sh`, `check-doc-links.py`), the model exporters
  (`blender/`), the art-pack scripts (`assets-pack.sh`, `fetch-assets.sh`, `upload-assets.sh`), `kernel-audit/`.
- Documents: `PLATFORM.md` (the contract), `MESHING.md` (the mesher), `structure.md` (the crate map), `docs/`.

## Licence

Code: either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option. Art: [CC BY-SA 4.0](LICENSE-ART).
Fonts, sound effects, dependencies and AI-generated media: see [NOTICE](NOTICE) and [THIRD-PARTY.md](THIRD-PARTY.md).
Contributions are accepted under the same terms ([CONTRIBUTING.md](CONTRIBUTING.md)).
