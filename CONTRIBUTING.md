# Contributing

Thank you for helping. This is a small project; a short issue before a big change saves everyone's time.

## Ground rules

* **English only** in everything public: code comments, documentation, commit messages, branch names, file names, issues.
  (Translations of the game texts live in `assets/lang/<code>/`.)
* Be kind and keep discussions on the work.
* Do not file security problems as public issues; use the repository's private vulnerability reporting.

## Licence of contributions

By submitting a contribution you agree that it is licensed under the terms of this repository: code under
"MIT OR Apache-2.0" (as in section 5 of the Apache License 2.0, inbound = outbound), art under CC BY-SA 4.0.
There is no CLA and no sign-off line is required. Do not submit code or media that you did not write or that has
an incompatible licence; if you add a dependency, add its licence to `THIRD-PARTY.md` (regenerate the table) and
mention it in the pull request.

## Workflow

1. Open an issue (bug or idea), or pick one.
2. Branch from `main`; name the branch `topic/short-description` (for example `fix/fog-edge`).
3. Keep commits small, with an imperative English subject line (`Fix the fog edge at the map border`) and a body that
   says why. Work done with an AI assistant carries a `Co-Authored-By:` trailer naming it.
4. Run the checks below, then open a pull request using the template.

## Checks before a pull request

Run the commands from the repository root.

```sh
cargo +1.98.1 check --workspace --all-targets
sh tools/platform-lint.sh          # the platform names no game module
sh tools/i18n-lint.sh              # no new raw user-visible strings; every key exists in English
python3 -I tools/check-doc-links.py
cargo +1.98.1 test --release -p aurelia-i18n -p aurelia-kernel -p aurelia-sandbox-rules -p aurelia-sandbox-content -p aurelia-net
```

* The game is deterministic: pinned hashes and snapshots in the tests fail when a rule changes. If you change a rule on
  purpose, update the pin in its own commit and say why.
* Rendering and UI changes need a screenshot in the pull request (`--screenshot out.png` renders without opening a window).
* The Rust toolchain is pinned (1.98.1); the tree is not yet rustfmt-clean, so do not reformat files you do not change.
