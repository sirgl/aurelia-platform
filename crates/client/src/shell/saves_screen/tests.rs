use std::cell::Cell;
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

use egui::Key;

use super::*;
use crate::shell::saves::{ModuleSaves, SaveSlot};

const CLASSIC: ModuleId = ModuleId { name: "classic", version: 1 };

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aurelia-saves-ui-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn age(path: &std::path::Path, seconds: u64) {
    let when = SystemTime::now() - Duration::from_secs(seconds);
    std::fs::File::options().write(true).open(path).unwrap().set_modified(when).unwrap();
}

/// A module that reads the first byte as the turn, refuses 0xFF as a newer format and 0xFE as damaged.
fn describe(payload: &[u8]) -> Result<SaveSummary, SaveProblem> {
    match payload.first() {
        Some(0xFF) => Err(SaveProblem::Newer(24)),
        Some(0xFE) => Err(SaveProblem::Damaged("save file is truncated".into())),
        Some(&turn) => Ok(SaveSummary { turn: Some(turn.into()), nation: Some("Brazil".into()), era: Some("Medieval Era".into()) }),
        None => Err(SaveProblem::Damaged("empty".into())),
    }
}

/// Manual "Brazil turn 12" (newest), the quick save, two autosaves, and a manual one a newer build wrote.
fn sample(root: &std::path::Path) -> ModuleSaves {
    let saves = ModuleSaves::under(root, CLASSIC);
    let named = saves.manual_slot("Brazil turn 12").unwrap();
    named.write(&[12]).unwrap();
    let newer = saves.manual_slot("From the future").unwrap();
    newer.write(&[0xFF]).unwrap();
    let quick = saves.quick_save();
    quick.write(&[9]).unwrap();
    let first = saves.autosaves().write(&[5], 5, 5).unwrap();
    let second = saves.autosaves().write(&[6], 6, 5).unwrap();
    age(named.path(), 30);
    age(newer.path(), 600);
    age(quick.path(), 7_200);
    age(second.path(), 90_000);
    age(first.path(), 200_000);
    saves
}

fn read_all(ui: &mut SavesUi) {
    assert!(!ui.read_pending(CLASSIC, &describe, Duration::from_secs(30)));
}

fn names(ui: &SavesUi) -> Vec<(SaveKind, String)> {
    ui.rows().iter().map(|r| (r.entry.kind, display_name(&r.entry))).collect()
}

#[test]
fn the_list_shows_every_kind_newest_first_with_their_headers() {
    let root = temp_dir("list");
    let saves = sample(&root);
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    assert!(ui.rows().iter().all(|r| r.header.is_none()), "nothing is read before the list asks");
    read_all(&mut ui);
    i18n::with_language("en", || {
        assert_eq!(
            names(&ui),
            vec![
                (SaveKind::Manual, "Brazil turn 12".to_owned()),
                (SaveKind::Manual, "From the future".to_owned()),
                (SaveKind::Quick, "Quick save".to_owned()),
                (SaveKind::Auto, "Autosave".to_owned()),
                (SaveKind::Auto, "Autosave".to_owned()),
            ]
        );
    });
    let turns: Vec<Option<u32>> = ui.rows().iter().map(|r| r.header.as_ref().and_then(|h| h.as_ref().ok()).and_then(|s| s.turn)).collect();
    assert_eq!(turns, vec![Some(12), None, Some(9), Some(6), Some(5)]);
    let summary = ui.rows()[0].header.as_ref().unwrap().as_ref().unwrap();
    assert_eq!((summary.nation.as_deref(), summary.era.as_deref()), (Some("Brazil"), Some("Medieval Era")));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_save_the_module_refuses_is_a_row_with_its_reason_and_cannot_be_loaded() {
    let root = temp_dir("unreadable");
    let saves = sample(&root);
    let broken = saves.manual_slot("Cut short").unwrap();
    broken.write(&[0xFE]).unwrap();
    std::fs::write(saves.manual_slot("Not a save").unwrap().path(), b"plain text, no tag").unwrap();
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    read_all(&mut ui);
    let words = i18n::with_language("en", crate::shell::words::current).saves;
    let reasons: Vec<String> = i18n::with_language("en", || {
        ui.rows().iter().filter_map(|r| r.header.as_ref().and_then(|h| h.as_ref().err()).map(|why| why.text(&words))).collect()
    });
    assert_eq!(reasons.len(), 3);
    assert!(reasons.contains(&"Saved by a newer version of the game (format 24)".to_owned()), "{reasons:?}");
    assert!(reasons.contains(&"The file is damaged: save file is truncated".to_owned()), "{reasons:?}");
    assert!(reasons.contains(&"This file does not name the rules it was saved with".to_owned()), "{reasons:?}");
    // Enter on an unreadable row does nothing; on a readable one it loads.
    let at = |ui: &SavesUi, name: &str| ui.rows().iter().position(|r| r.entry.name == name).unwrap();
    ui.select(at(&ui, "From the future"));
    assert_eq!(ui.key(Key::Enter), None);
    ui.select(at(&ui, "Brazil turn 12"));
    assert_eq!(ui.key(Key::Enter), Some(ShellAction::LoadSave(saves.manual_slot("Brazil turn 12").unwrap().path().to_path_buf())));
    // Delete still works on the unreadable one.
    ui.select(at(&ui, "From the future"));
    assert_eq!(ui.key(Key::Delete), None);
    assert_eq!(ui.confirming(), Some(&saves.manual_slot("From the future").unwrap().path().to_path_buf()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn headers_are_read_once_per_file_time_and_a_rewritten_save_is_read_again() {
    let root = temp_dir("cache");
    let saves = sample(&root);
    let calls = Cell::new(0);
    let counting = |payload: &[u8]| {
        calls.set(calls.get() + 1);
        describe(payload)
    };
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    assert!(!ui.read_pending(CLASSIC, &counting, Duration::from_secs(30)));
    assert_eq!(calls.get(), 5);
    ui.open_list(saves.entries());
    assert!(ui.rows().iter().all(|r| r.header.is_some()), "the second opening has every header at once");
    assert!(!ui.read_pending(CLASSIC, &counting, Duration::from_secs(30)));
    assert_eq!(calls.get(), 5, "nothing was read again");
    // The player saves over a name: a new time, a new read.
    let slot = saves.manual_slot("Brazil turn 12").unwrap();
    slot.write(&[40]).unwrap();
    age(slot.path(), 1);
    ui.open_list(saves.entries());
    assert!(ui.rows().iter().any(|r| r.header.is_none()));
    read_all(&mut ui);
    let turn = ui.rows().iter().find(|r| r.entry.name == "Brazil turn 12").and_then(|r| r.header.clone()).unwrap().unwrap().turn;
    assert_eq!(turn, Some(40));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_frame_reads_only_as_many_rows_as_its_time_allows() {
    let root = temp_dir("budget");
    let saves = sample(&root);
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    assert!(ui.read_pending(CLASSIC, &describe, Duration::ZERO), "no time, nothing read, rows wait");
    assert!(ui.rows().iter().all(|r| r.header.is_none()));
    read_all(&mut ui);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn keys_move_the_selection_and_ask_for_loads_and_confirmed_deletes() {
    let root = temp_dir("keys");
    let saves = sample(&root);
    let mut ui = SavesUi::default();
    assert_eq!(ui.key(Key::Enter), None, "a closed list takes no keys");
    ui.open_list(saves.entries());
    read_all(&mut ui);
    assert_eq!(ui.selection_index(), Some(0));
    ui.key(Key::ArrowDown);
    ui.key(Key::ArrowDown);
    assert_eq!(ui.selected().unwrap().entry.kind, SaveKind::Quick);
    ui.key(Key::ArrowUp);
    ui.key(Key::ArrowUp);
    ui.key(Key::ArrowUp);
    assert_eq!(ui.selection_index(), Some(0), "the top stops it");
    ui.key(Key::End);
    assert_eq!(ui.selection_index(), Some(4));
    ui.key(Key::ArrowDown);
    assert_eq!(ui.selection_index(), Some(4), "so does the bottom");
    let oldest = ui.selected().unwrap().entry.path.clone();
    assert_eq!(ui.key(Key::Enter), Some(ShellAction::LoadSave(oldest.clone())));

    // Delete asks first; Esc takes the question back, then closes the list.
    assert_eq!(ui.key(Key::Delete), None);
    assert_eq!(ui.confirming(), Some(&oldest));
    assert!(ui.cancel());
    assert_eq!(ui.confirming(), None);
    assert!(ui.is_open(), "the first Esc only withdrew the question");
    ui.key(Key::Delete);
    ui.key(Key::ArrowUp);
    assert_eq!(ui.confirming(), None, "moving away withdraws it too");
    ui.key(Key::End);
    ui.key(Key::Delete);
    assert_eq!(ui.key(Key::Enter), Some(ShellAction::DeleteSave(oldest)), "Enter on the question says yes");
    assert!(ui.cancel() && !ui.is_open());
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_through_the_list_removes_the_file_and_the_selection_stays_close() {
    let root = temp_dir("delete");
    let saves = sample(&root);
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    read_all(&mut ui);
    ui.select(3);
    let path = ui.selected().unwrap().entry.path.clone();
    saves.delete(&path).unwrap();
    ui.reload(saves.entries());
    assert_eq!(ui.rows().len(), 4);
    assert!(ui.rows().iter().all(|r| r.entry.path != path));
    assert_eq!(ui.selection_index(), Some(3), "the next row takes the place");
    assert!(ui.rows().iter().all(|r| r.header.is_some()), "the others keep what was read");
    // The last row goes: the selection moves up; an empty list selects nothing.
    ui.select(3);
    let last = ui.selected().unwrap().entry.path.clone();
    saves.delete(&last).unwrap();
    ui.reload(saves.entries());
    assert_eq!(ui.selection_index(), Some(2));
    for row in saves.entries() {
        saves.delete(&row.path).unwrap();
    }
    ui.reload(saves.entries());
    assert!(ui.rows().is_empty() && ui.selected().is_none());
    assert_eq!(ui.key(Key::Enter), None);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn the_naming_dialog_defaults_to_the_nation_and_turn_and_says_when_a_name_is_taken() {
    let mut ui = SavesUi::default();
    i18n::with_language("en", || {
        let summary = SaveSummary { nation: Some("Brazil".into()), ..Default::default() };
        assert_eq!(default_name(Some(&summary), 120), "Brazil turn 120");
        assert_eq!(default_name(None, 7), "Turn 7");
        assert_eq!(default_name(Some(&SaveSummary::default()), 7), "Turn 7");
    });
    ui.open_name("Brazil turn 120".into(), vec!["brazil turn 120".into()]);
    assert_eq!(ui.name(), Some("Brazil turn 120"));
    assert_eq!(ui.key(Key::Enter), Some(ShellAction::SaveAs("Brazil turn 120".into())));
    ui.set_name("   ");
    assert_eq!(ui.key(Key::Enter), None, "a name with nothing to name a file by saves nothing");
    ui.set_name("  a/b  ");
    assert_eq!(ui.key(Key::Enter), Some(ShellAction::SaveAs("a/b".into())), "the app cleans the name for the file");
    assert!(ui.cancel() && !ui.is_open());
}

#[test]
fn ages_are_said_in_the_nearest_unit_and_a_month_on_as_a_date() {
    i18n::with_language("en", || {
        assert_eq!(age_text(1000, 1000), "Just now");
        assert_eq!(age_text(1000, 941), "Just now");
        assert_eq!(age_text(1000, 940), "1 minute ago");
        assert_eq!(age_text(10_000, 7_000), "50 minutes ago");
        assert_eq!(age_text(10_000, 6_400), "1 hour ago");
        assert_eq!(age_text(200_000, 20_000), "2 days ago");
        assert_eq!(age_text(1_793_000_000, 1_790_000_000), "9/21/2026");
        assert_eq!(age_text(5, 100), "Just now", "a file from the future is not an error");
    });
    i18n::with_language("ru", || {
        assert_eq!(age_text(10_000, 10_000 - 3 * 60), "3 минуты назад");
        assert_eq!(age_text(10_000, 10_000 - 5 * 60), "5 минут назад");
        assert_eq!(age_text(1_793_000_000, 1_790_000_000), "21.09.2026");
    });
    assert_eq!(civil_date(0), (1970, 1, 1));
    assert_eq!(civil_date(951_782_400), (2000, 2, 29));
    assert_eq!(civil_date(1_793_000_000), (2026, 10, 26));
}

#[test]
fn another_modules_save_in_the_folder_is_an_unreadable_row() {
    let root = temp_dir("other");
    let saves = ModuleSaves::under(&root, CLASSIC);
    SaveSlot::at(saves.manual_slot("Foreign").unwrap().path().to_path_buf(), ModuleId { name: "other", version: 1 }).write(&[1]).unwrap();
    let mut ui = SavesUi::default();
    ui.open_list(saves.entries());
    read_all(&mut ui);
    let words = i18n::with_language("en", crate::shell::words::current).saves;
    let why = ui.rows()[0].header.as_ref().unwrap().as_ref().unwrap_err().text(&words);
    assert_eq!(why, "This save was made with the other rules; this game runs the classic rules");
    std::fs::remove_dir_all(root).unwrap();
}
