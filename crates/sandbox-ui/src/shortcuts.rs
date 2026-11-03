//! The keyboard of the sandbox module: found a city. The rest (end turn, camera) is the shell's.
use client::shell::keymap::{ActionDef, Scope, ShortcutSet};

pub const SET: ShortcutSet = ShortcutSet { name: "sandbox", rows: ROWS };

pub const ROWS: &[ActionDef] = &[ActionDef::new("order-found-city", "units", Scope::Map, "B")];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_an_english_label_and_a_group() {
        for def in ROWS {
            assert!(i18n::tr_dynamic(&def.label_key(), &[]).is_some(), "{} has no message", def.label_key());
            assert!(i18n::tr_dynamic(&format!("shortcut.group.{}", def.group), &[]).is_some(), "group {}", def.group);
        }
    }
}
