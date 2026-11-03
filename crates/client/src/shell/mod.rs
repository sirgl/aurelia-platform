//! The app shell: everything a game needs on screen that names no mechanic. The frame
//! (`app`), the screen stack and menu (`screens`), the HUD frame over the map (`hud`),
//! the presentation of what the simulation did (`presentation`), settings, saves, art and
//! the look of the screens and the widgets modules build on (`kit`, `art`, `widgets`). A game module plugs in through `module`.
pub mod app;
pub mod art;
mod art_loader;
pub mod assets;
pub mod capture;
pub mod device;
pub mod fold;
pub mod gpu;
pub mod hover;
pub mod hud;
pub mod keymap;
pub mod language;
pub mod kit;
pub mod layout;
pub mod map_feedback;
pub mod module;
pub mod observer;
pub mod pacing;
pub mod presentation;
pub mod report;
pub mod saves;
pub mod saves_screen;
pub mod screens;
pub mod scroll;
pub mod settings;
pub mod storage;
pub mod task;
mod settings_screen;
pub mod shortcuts;
pub mod snapshot;
pub mod theme;
pub mod touch;
pub mod widgets;
pub mod words;
