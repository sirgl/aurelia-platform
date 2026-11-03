//! Pure dependency planning. Costs, grants and technology effects stay in research.
use crate::ids::TechId;
use crate::rules::CompiledRuleset;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchPlanError {
    UnknownTechnology(TechId),
    AlreadyResearched(TechId),
    DependencyCycle(TechId),
}
impl std::fmt::Display for ResearchPlanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownTechnology(id) => write!(f, "unknown technology {}", id.0),
            Self::AlreadyResearched(id) => write!(f, "technology {} already researched", id.0),
            Self::DependencyCycle(id) => write!(f, "research dependency cycle at {}", id.0),
        }
    }
}
impl std::error::Error for ResearchPlanError {}

/// Every missing prerequisite once, in deterministic dependency order; target last.
/// Known nodes terminate traversal. O(V+E) time and O(V) memory; no recursion.
/// Does not consume free technology choices or accumulated science.
pub fn plan_research<M: crate::module::RulesModule>(
    rules: &CompiledRuleset<M>,
    known: &[TechId],
    target: TechId,
) -> Result<Vec<TechId>, ResearchPlanError> {
    let mut marks = vec![0u8; rules.technologies.len()];
    if target.0 as usize >= marks.len() {
        return Err(ResearchPlanError::UnknownTechnology(target));
    }
    for &id in known {
        *marks
            .get_mut(id.0 as usize)
            .ok_or(ResearchPlanError::UnknownTechnology(id))? = 2;
    }
    if marks[target.0 as usize] == 2 {
        return Err(ResearchPlanError::AlreadyResearched(target));
    }
    let mut pending = vec![(target, 0usize)];
    let mut plan = Vec::new();
    marks[target.0 as usize] = 1;
    while let Some((id, next)) = pending.last_mut() {
        if let Some(&dependency) = rules.technologies[id.0 as usize].requires.get(*next) {
            *next += 1;
            let mark = marks
                .get_mut(dependency.0 as usize)
                .ok_or(ResearchPlanError::UnknownTechnology(dependency))?;
            match *mark {
                1 => return Err(ResearchPlanError::DependencyCycle(dependency)),
                2 => {}
                _ => {
                    *mark = 1;
                    pending.push((dependency, 0));
                }
            }
        } else {
            marks[id.0 as usize] = 2;
            plan.push(*id);
            pending.pop();
        }
    }
    Ok(plan)
}
