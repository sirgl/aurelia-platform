//! Widgets a module's screens and HUD are built from, beyond the small ones in `kit`: a
//! graph of cards with prerequisites (`dag`), an image card (`cards`), a list of notices
//! (`notices`) and the end of a game (`game_end`). Each takes a typed view model that
//! names no mechanic and returns what the player did; the words, the placement and the
//! meaning of a click stay the module's.
//!
//! How a module uses them (Lands & Laws included):
//! - Technology tree: build a `dag::Graph` (one `Node` per technology: `Cell` from the
//!   content or `dag::layered` over the prerequisites, `Progress`, a `cards::ImageCard`
//!   with the art, the name and a status line), keep a `dag::GraphState` between frames,
//!   call `dag::show` with an `art::ArtPen`; act on `clicked`, add tooltips to `cards`.
//! - HUD strip (current research, a goal): paint a `cards::ImageCard` with
//!   `cards::STRIP` in the module's `egui::Area`.
//! - Notices and the agenda: `notices::show` with `Notice` rows; `Action::Row` for "show
//!   on the map", `Action::Button` for an agenda item's button.
//! - Collapsible panel: keep its state in `shell::fold` (`is_open`, `toggle`; the settings
//!   remember it), draw `fold::header` and, when open, the body under it. `notices::show_clipped`
//!   cuts long lines to the panel's width.
//! - A dock of round buttons with counters: `hud::round_button` and `dock::badge`; the research
//!   strip as a popup or pinned: `research_strip::research_strip`.
//! - End of the game: `game_end::announce` on the map (a HUD layer), `game_end::show` on
//!   the results screen with the Hall of Fame.
pub mod cards;
pub mod dag;
pub mod dock;
pub mod fold;
pub mod game_end;
pub mod notices;
pub mod research_strip;
