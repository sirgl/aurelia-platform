//! Where the app keeps the player's games, apart per rules module (`ModuleSaves`): the
//! quick save, the ring of autosaves with their command journals, the saves the player named
//! (`list`) and the Hall of Fame.
//! Every save names the module that wrote it; the bytes after that name come from the
//! module, nothing here reads them.
pub mod autosave;
pub mod hall_of_fame;
pub mod header;
pub mod journal;
pub mod list;
pub mod module_saves;
pub mod rules;
pub mod slot;

pub use autosave::{latest_save, Autosaves};
pub use hall_of_fame::HallOfFame;
pub use header::{read_header, SaveProblem, SaveSummary, Unreadable};
pub use list::{file_stem, SaveEntry, SaveKind, NAME_LIMIT};
pub use journal::{journal_path, CommandJournal, JournalFile};
pub use module_saves::ModuleSaves;
pub use rules::{drift_notice, rules_match, rules_notice, SavesStage};
pub use slot::{read_tagged, save_directory, tagged, SaveError, SaveSlot};
