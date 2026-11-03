//! Which rules a loaded save may run under. A released module refuses a save of
//! other rules (another pack, another build) with a typed error
//! (`RulesMatch::SameRules`); a module in development loads it under the current
//! rules and says which parts of the rules changed (`RulesMatch::AnyRules`), so a
//! game goes on across builds. `AURELIA_DEV_SAVES=1` or `=0` overrides the module's
//! stage (platform/docs/saves.md).
use kernel::persistence::decode::{RulesCheck, RulesDrift, RulesMatch};

/// The variable that overrides the module's stage: 1 development, 0 release.
pub const DEV_SAVES: &str = "AURELIA_DEV_SAVES";

/// Where a module is on its way to a release: what its loads accept by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SavesStage {
    /// Saves of other rules are refused (the classic module).
    Release,
    /// Saves of other rules load under the current rules (Lands & Laws before 1.0).
    Development,
}

/// What this run accepts for a module at `stage`.
pub fn rules_match(stage: SavesStage) -> RulesMatch {
    match_of(stage, std::env::var(DEV_SAVES).ok().as_deref())
}

fn match_of(stage: SavesStage, dev_saves: Option<&str>) -> RulesMatch {
    match (dev_saves, stage) {
        (Some("1"), _) | (None, SavesStage::Development) => RulesMatch::AnyRules,
        _ => RulesMatch::SameRules,
    }
}

/// The line the shell shows after a load (None: the save's own rules).
pub fn rules_notice(check: &RulesCheck) -> Option<String> {
    match check {
        RulesCheck::Same => None,
        RulesCheck::Changed(drift) => Some(drift_notice(drift)),
    }
}

/// "Loaded under the current rules; changed: units, technologies".
pub fn drift_notice(drift: &RulesDrift) -> String {
    let changed = match &drift.parts {
        Some(parts) if !parts.is_empty() => i18n::tf!("shell.saves.drift-changed", parts = parts.join(", ")),
        Some(_) => i18n::t!("shell.saves.drift-none").to_owned(),
        None => i18n::t!("shell.saves.drift-unknown").to_owned(),
    };
    i18n::tf!("shell.saves.drift", saved = format!("{:016x}", drift.saved), current = format!("{:016x}", drift.current), changed = changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stage_decides_unless_the_variable_overrides_it() {
        use SavesStage::{Development, Release};
        assert_eq!(match_of(Release, None), RulesMatch::SameRules);
        assert_eq!(match_of(Development, None), RulesMatch::AnyRules);
        assert_eq!(match_of(Release, Some("1")), RulesMatch::AnyRules);
        assert_eq!(match_of(Development, Some("0")), RulesMatch::SameRules);
    }

    #[test]
    fn the_notice_names_the_changed_parts() {
        assert_eq!(rules_notice(&RulesCheck::Same), None);
        let drift = |parts| RulesDrift { saved: 0xa, current: 0xb, parts, shifted: None };
        assert_eq!(
            drift_notice(&drift(Some(vec!["units".into(), "lal tables after handicaps".into()]))),
            "Saved with other rules (000000000000000a); loaded under the current rules (000000000000000b); \
             changed: units, lal tables after handicaps"
        );
        assert!(drift_notice(&drift(None)).ends_with("the save does not say which parts changed"));
    }
}
