//! Compilation of the difficulty levels: keys and
//! level indexes unique, rows ordered by level. No other table refers to
//! a handicap; the game setup picks one by key.

use crate::module::RulesModule;
use std::collections::HashSet;

use crate::rules::compile::RulesError;
use crate::rules::compiled::HandicapDefinition;
use crate::rules::source::SourceRules;

pub(crate) fn compile_handicaps<M: RulesModule>(s: &SourceRules<M>) -> Result<Vec<HandicapDefinition<M>>, RulesError> {
    let mut out = Vec::with_capacity(s.handicaps.len());
    let mut keys = HashSet::new();
    let mut levels = HashSet::new();
    for row in &s.handicaps {
        let Some(id) = &row.id else {
            return Err(RulesError::Inconsistent(format!(
                "handicap {} has no key",
                row.name
            )));
        };
        if !keys.insert(id.0.clone()) || !levels.insert(row.index) {
            let reason = format!("{}: handicap key or level {} used twice", id.0, row.index);
            return Err(RulesError::Inconsistent(reason));
        }
        out.push(HandicapDefinition {
            key: id.0.clone(),
            name: row.name.clone(),
            index: row.index,
            values: row.values,
        });
    }
    out.sort_by_key(|h| h.index);
    Ok(out)
}
