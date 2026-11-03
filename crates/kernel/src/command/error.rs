/// Why an order is refused in the cases every rules module has. A user interface words each
/// of them once from the type (no parsing of rule text); `english` is the rule text
/// of logs, tests and modules without such a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The unit has no move points for the order.
    NoMovesLeft,
    /// The destination lies in territory the unit's owner may not enter.
    ClosedBorders,
    AlreadyFortified,
    /// The price is more than the treasury holds.
    NotEnoughGold { price: u32, treasury: u32 },
    /// The order needs a city that builds something.
    NothingQueued,
    TileNotForSale,
    /// The worker is busy with a job.
    AlreadyWorking,
    /// A settler may not found a city here.
    Site(SiteProblem),
    /// A city name that is no choice: it is not on the nation's list or another city
    /// holds it.
    NameTaken,
    /// A unit or building whose technology the player does not know.
    NeedsTechnology(crate::ids::TechId),
    /// The city has built it already.
    AlreadyBuilt,
    /// The city has it in its queue already.
    AlreadyQueued,
    /// A world or national wonder another own city has in its queue.
    QueuedElsewhere,
    /// A wonder some city has built.
    WonderBuilt,
    /// Unique to another nation.
    OtherNation,
    /// Content that production never makes (great people, the palace).
    NotProducible,
}

/// Why a city cannot be founded on a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteProblem {
    /// Water, mountains and other land nobody can settle.
    Unsettleable,
    ForeignTerritory,
    /// Another city is within the minimum distance.
    TooClose,
}

impl SiteProblem {
    pub fn english(self) -> &'static str {
        match self {
            SiteProblem::Unsettleable => "cannot settle here",
            SiteProblem::ForeignTerritory => "foreign territory",
            SiteProblem::TooClose => "too close to a city",
        }
    }
}

impl Refusal {
    pub fn english(self) -> &'static str {
        match self {
            Refusal::NoMovesLeft => "exhausted",
            Refusal::ClosedBorders => "closed borders",
            Refusal::AlreadyFortified => "already fortified",
            Refusal::NotEnoughGold { .. } => "not enough gold",
            Refusal::NothingQueued => "queue empty",
            Refusal::TileNotForSale => "tile not purchasable",
            Refusal::AlreadyWorking => "already working",
            Refusal::Site(problem) => problem.english(),
            Refusal::NameTaken => "name not available",
            Refusal::NeedsTechnology(_) => "technology required",
            Refusal::AlreadyBuilt => "already built",
            Refusal::AlreadyQueued => "already queued",
            Refusal::QueuedElsewhere => "queued in another city",
            Refusal::WonderBuilt => "wonder already built",
            Refusal::OtherNation => "unique to another nation",
            Refusal::NotProducible => "not producible",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    UnknownUnit,
    UnknownPlayer,
    UnknownCity,
    NotOwner,
    BadDestination,
    Impassable,
    RuleViolation(String),
    /// A refusal the rules name by type (`Refusal`).
    Refused(Refusal),
    Internal(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Refused(refusal) => f.write_str(refusal.english()),
            other => write!(f, "{other:?}"),
        }
    }
}

impl std::error::Error for CommandError {}
