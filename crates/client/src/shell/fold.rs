//! Which collapsible panels are open, by name. The state lives in the egui context
//! (`ctx.data`), so a panel asks for it wherever it is drawn; the app loads it from the
//! settings at start and writes it back after a frame that changed it (`export`).
//! Everything is folded by default.
use egui::{Context, Id};
use std::collections::BTreeSet;

/// Longest panel name and most open panels the settings keep.
const NAME_MAX: usize = 40;
const OPEN_MAX: usize = 64;

#[derive(Clone, Default)]
struct Folds {
    open: BTreeSet<String>,
    changed: bool,
}

fn id() -> Id {
    Id::new("shell_fold_state")
}

fn with<R>(ctx: &Context, f: impl FnOnce(&mut Folds) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<Folds>(id())))
}

/// A panel name the settings file can hold: `[a-z0-9_]`, 1..=40 characters.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= NAME_MAX && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// The valid, distinct, sorted names of a list (at most 64).
pub fn sanitize<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let set: BTreeSet<&str> = names.into_iter().filter(|n| valid_name(n)).collect();
    set.into_iter().take(OPEN_MAX).map(str::to_owned).collect()
}

/// The panel is open (folded when never opened).
pub fn is_open(ctx: &Context, name: &str) -> bool {
    with(ctx, |s| s.open.contains(name))
}

/// Open or fold a panel; a real change is reported once by `take_changed`.
pub fn set_open(ctx: &Context, name: &str, open: bool) {
    with(ctx, |s| {
        let changed = if open { s.open.insert(name.to_owned()) } else { s.open.remove(name) };
        s.changed |= changed;
    });
}

/// Flip a panel; returns whether it is open now.
pub fn toggle(ctx: &Context, name: &str) -> bool {
    let open = !is_open(ctx, name);
    set_open(ctx, name, open);
    open
}

/// Replace the open panels with a saved list (not a change to write back).
pub fn load(ctx: &Context, names: &[String]) {
    with(ctx, |s| {
        s.open = names.iter().cloned().collect();
        s.changed = false;
    });
}

/// The open panels, sorted.
pub fn open_list(ctx: &Context) -> Vec<String> {
    with(ctx, |s| s.open.iter().cloned().collect())
}

/// True once after panels were opened or folded since the last call.
pub fn take_changed(ctx: &Context) -> bool {
    with(ctx, |s| std::mem::take(&mut s.changed))
}

/// Copy the open panels into `saved` when they changed since the last call; true when
/// `saved` now differs (the caller persists it).
pub fn export(ctx: &Context, saved: &mut Vec<String>) -> bool {
    if !take_changed(ctx) {
        return false;
    }
    let now = open_list(ctx);
    let differs = *saved != now;
    *saved = now;
    differs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_start_folded_and_toggle() {
        let ctx = Context::default();
        assert!(!is_open(&ctx, "events"));
        assert!(!take_changed(&ctx));
        assert!(toggle(&ctx, "events"));
        assert!(is_open(&ctx, "events"));
        assert!(!toggle(&ctx, "events"));
        assert!(take_changed(&ctx), "opened and folded again still counts as a change");
        assert!(!take_changed(&ctx), "reported once");
        set_open(&ctx, "events", false);
        assert!(!take_changed(&ctx), "folding a folded panel changes nothing");
    }

    #[test]
    fn open_list_is_sorted_and_load_is_not_a_change() {
        let ctx = Context::default();
        load(&ctx, &["b".to_owned(), "a".to_owned()]);
        assert!(!take_changed(&ctx));
        assert_eq!(open_list(&ctx), ["a", "b"]);
        assert!(is_open(&ctx, "a"));
    }

    #[test]
    fn names_are_checked() {
        assert!(valid_name("city_list2"));
        for bad in ["", "Events", "a-b", "a b", "a,b", "é", &"x".repeat(41)] {
            assert!(!valid_name(bad), "{bad:?}");
        }
        assert_eq!(sanitize(["b", "a", "b", "BAD", ""]), ["a", "b"]);
    }

    #[test]
    fn export_writes_back_only_real_changes() {
        let ctx = Context::default();
        let mut saved = Vec::new();
        assert!(!export(&ctx, &mut saved));
        set_open(&ctx, "events", true);
        assert!(export(&ctx, &mut saved));
        assert_eq!(saved, ["events"]);
        set_open(&ctx, "events", false);
        set_open(&ctx, "events", true);
        assert!(!export(&ctx, &mut saved), "back to the saved state");
    }
}
