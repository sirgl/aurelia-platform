//! A city's economy stocks besides population and food (authoritative,
//! saved, hashed): the food kept for the next growth and what the city
//! does with its production when its queue is empty. The rules module's
//! own stocks of a city are its city state (`world::module::ModuleCity`).

use crate::rules::content::ProjectId;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CityEconomy {
    /// Food kept on growth (whole food): granary-like
    /// buildings keep a share of the threshold (`Stat::FoodKept`).
    pub food_kept: u32,
    /// What the city does with its production when its queue is empty.
    pub process: Option<Process>,
}

/// A production process (wealth, research, and the World Congress
/// projects' conversions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Process {
    Wealth,
    Research,
    /// The city's production goes to a project the players contribute to
    /// (`ProjectFacts::contributed`; Classic: a World Congress project).
    Contribute(ProjectId),
}

/// Save code of the first contribution process (`3 + project id`).
const CONTRIBUTE_CODE: u8 = 3;

impl Process {
    /// The processes a technology unlocks.
    pub const UNLOCKED: [Process; 2] = [Process::Wealth, Process::Research];

    /// The role a technology unlocks it by (None: a contribution,
    /// available while its project is under way).
    pub fn role(self) -> Option<crate::rules::roles::ProcessRole> {
        match self {
            Process::Wealth => Some(crate::rules::roles::ProcessRole::Wealth),
            Process::Research => Some(crate::rules::roles::ProcessRole::Research),
            Process::Contribute(_) => None,
        }
    }

    /// Save code (0 is "no process").
    pub fn code(process: Option<Process>) -> u8 {
        match process {
            None => 0,
            Some(Process::Wealth) => 1,
            Some(Process::Research) => 2,
            Some(Process::Contribute(project)) => CONTRIBUTE_CODE.saturating_add(project.0.min(250) as u8),
        }
    }

    pub fn from_code(code: u8) -> Result<Option<Process>, ()> {
        match code {
            0 => Ok(None),
            1 => Ok(Some(Process::Wealth)),
            2 => Ok(Some(Process::Research)),
            n => Ok(Some(Process::Contribute(ProjectId(u16::from(n - CONTRIBUTE_CODE))))),
        }
    }
}
