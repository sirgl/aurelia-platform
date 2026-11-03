use crate::module::RulesModule;
use crate::ids::{PlayerId, TechId};
use crate::rules::CompiledRuleset;

/// Outcome of learning a technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearnOutcome {
    /// Learned for the first time.
    Learned,
    /// Already known (a repeat is a command error, not silence).
    AlreadyKnown,
    /// No such player.
    UnknownPlayer,
}

/// What kind of player this is: a major nation, or one of the kinds
/// the rules module adds (Classic: a city-state, the barbarians). Every
/// player takes its turns alike; the module's rules tell the kinds apart,
/// the kernel asks a kind only what its own rules need
/// ([`ModulePlayerKind`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PlayerKind<M: RulesModule> {
    #[default]
    Major,
    Module(ModulePlayerKindOf<M>),
}

/// The kinds of player a rules module adds (`ModuleWorld::PlayerKind`).
pub type ModulePlayerKindOf<M> = <<M as RulesModule>::World as crate::world::module::ModuleWorld<M>>::PlayerKind;

/// What the kernel asks of a kind of player the rules module adds.
pub trait ModulePlayerKind<M: RulesModule>: Copy + std::fmt::Debug + PartialEq + Eq + std::hash::Hash + 'static {
    /// Its save code: a tag of 1 or more (0 is a major nation) and
    /// an id. The world hash mixes it too.
    fn code(self) -> (u8, u16);

    /// The kind a save code names (None: no such kind in these rules).
    fn from_code(tag: u8, id: u16, rules: &CompiledRuleset<M>) -> Option<Self>;

    /// The name every city of the player takes (Classic: a city-state's
    /// name); None: the names of its nation.
    fn city_name(self, rules: &CompiledRuleset<M>) -> Option<String>;

    /// At war with every other player from the start (Classic: barbarians).
    fn always_at_war(self) -> bool;

    /// Trains units that found cities (Classic: not a city-state).
    fn trains_founders(self) -> bool;

    /// Its land hosts other players' improvements that may stand only on
    /// such land (`ImprovementPlacement::only_host_land`; Classic: a
    /// city-state hosts the Feitoria).
    fn hosts_improvements(self) -> bool;

    /// Its land is open to every player's units in peace, without an
    /// open-borders grant (Classic: a city-state).
    fn open_territory(self) -> bool;
}

/// A module without kinds of its own: every player is a major nation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoKind {}

impl<M: RulesModule> ModulePlayerKind<M> for NoKind {
    fn code(self) -> (u8, u16) {
        match self {}
    }
    fn from_code(_: u8, _: u16, _: &CompiledRuleset<M>) -> Option<Self> {
        None
    }
    fn city_name(self, _: &CompiledRuleset<M>) -> Option<String> {
        match self {}
    }
    fn always_at_war(self) -> bool {
        match self {}
    }
    fn trains_founders(self) -> bool {
        match self {}
    }
    fn hosts_improvements(self) -> bool {
        match self {}
    }
    fn open_territory(self) -> bool {
        match self {}
    }
}

impl<M: RulesModule> PlayerKind<M> {
    /// Save code (0 for a major nation, `ModulePlayerKind::code`).
    pub fn code(self) -> (u8, u16) {
        match self {
            PlayerKind::Major => (0, 0),
            PlayerKind::Module(kind) => kind.code(),
        }
    }

    /// The kind a save code names (None: no such kind in these rules).
    pub fn from_code((tag, id): (u8, u16), rules: &CompiledRuleset<M>) -> Option<Self> {
        match tag {
            0 => Some(PlayerKind::Major),
            _ => ModulePlayerKind::from_code(tag, id, rules).map(PlayerKind::Module),
        }
    }

    /// The module's kind (None: a major nation).
    pub fn module(self) -> Option<ModulePlayerKindOf<M>> {
        match self {
            PlayerKind::Major => None,
            PlayerKind::Module(kind) => Some(kind),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlayerCore<M: RulesModule> {
    pub alive: bool,
    pub techs: Vec<TechId>,
    /// Empire treasury (gold): income from citizens, expenses are hurry/upkeep.
    pub treasury: u32,
    /// Beakers (science): income from citizens, spent on completing research.
    pub science: u32,
    /// Research queue (the head is being learned; overflow goes after).
    pub research_queue: Vec<TechId>,
    /// Nation index (rules.nations; 0 = nations[0] / an old save without the field).
    pub nation: u8,
    /// Tiles the player has bought (raises the
    /// price of the next).
    pub plots_bought: u16,
    /// The rules module's state of the player (`ModuleWorld::Player`; Nation
    /// V: golden ages, great people, happiness snapshot, owed choices).
    pub economy: crate::world::module::ModulePlayerOf<M>,
    pub kind: PlayerKind<M>,
}

#[derive(Debug, Clone, Default)]
pub struct PlayerStore<M: RulesModule> {
    players: Vec<PlayerCore<M>>,
}

/// Technologies a research queue holds at most: a full dependency plan
/// may contain every representable technology once.
pub const RESEARCH_QUEUE_MAX: usize = u16::MAX as usize + 1;

impl<M: RulesModule> PlayerStore<M> {
    pub fn new(count: u8) -> Self {
        Self {
            players: vec![
                PlayerCore {
                    alive: true,
                    techs: vec![],
                    treasury: 0,
                    science: 0,
                    research_queue: vec![],
                    nation: 0,
                    plots_bought: 0,
                    economy: Default::default(),
                    kind: PlayerKind::Major,
                };
                count as usize
            ],
        }
    }

    pub fn count(&self) -> usize {
        self.players.len()
    }

    pub fn is_alive(&self, p: PlayerId) -> bool {
        self.players
            .get(p.0 as usize)
            .map(|c| c.alive)
            .unwrap_or(false)
    }

    pub fn techs_of(&self, p: PlayerId) -> &[TechId] {
        self.players
            .get(p.0 as usize)
            .map(|c| c.techs.as_slice())
            .unwrap_or(&[])
    }

    /// Learn a technology. Three outcomes instead of a bool: "already known" and "no
    /// such player" are different errors further up the stack.
    /// The only mutation of techs (transactions + restore).
    pub fn learn_tech(&mut self, p: PlayerId, tech: TechId) -> LearnOutcome {
        match self.players.get_mut(p.0 as usize) {
            None => LearnOutcome::UnknownPlayer,
            Some(core) if core.techs.contains(&tech) => LearnOutcome::AlreadyKnown,
            Some(core) => {
                core.techs.push(tech);
                LearnOutcome::Learned
            }
        }
    }

    /// Set the alive flag. Called only by the transaction commit after the
    /// reconciliation (a city or a unit exists: alive); we do not touch the flags directly.
    pub fn set_alive(&mut self, p: PlayerId, alive: bool) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.alive = alive;
        }
    }

    /// Treasury gold (0 for unknown: cannot happen outside the simulation).
    pub fn treasury_of(&self, p: PlayerId) -> u32 {
        self.players
            .get(p.0 as usize)
            .map(|c| c.treasury)
            .unwrap_or(0)
    }

    /// Income into the treasury (saturating: overflow is impossible).
    pub fn earn(&mut self, p: PlayerId, amount: u32) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.treasury = core.treasury.saturating_add(amount);
        }
    }

    /// Spend from the treasury: all or nothing (there are no partial write-offs,
    /// otherwise the economy drifts into "almost enough").
    pub fn spend(&mut self, p: PlayerId, amount: u32) -> bool {
        match self.players.get_mut(p.0 as usize) {
            Some(core) if core.treasury >= amount => {
                core.treasury -= amount;
                true
            }
            _ => false,
        }
    }

    /// Upkeep: takes what there is, returns the shortfall.
    /// A deficit has no consequences yet (disbanding units is a gap, not a bug).
    /// The treasury after the turn's balance (`systems::economy`).
    pub(crate) fn set_treasury(&mut self, p: PlayerId, gold: u32) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.treasury = gold;
        }
    }

    #[allow(dead_code)]
    pub fn drain(&mut self, p: PlayerId, amount: u32) -> u32 {
        match self.players.get_mut(p.0 as usize) {
            Some(core) => {
                let paid = core.treasury.min(amount);
                core.treasury -= paid;
                amount - paid
            }
            None => amount,
        }
    }

    /// Beakers in the pool (0 for unknown).
    pub fn science_of(&self, p: PlayerId) -> u32 {
        self.players
            .get(p.0 as usize)
            .map(|c| c.science)
            .unwrap_or(0)
    }

    /// Research queue (empty = nothing is being learned).
    pub fn research_queue_of(&self, p: PlayerId) -> &[TechId] {
        self.players
            .get(p.0 as usize)
            .map(|c| c.research_queue.as_slice())
            .unwrap_or(&[])
    }

    /// Beaker income (saturating).
    pub fn earn_science(&mut self, p: PlayerId, amount: u32) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.science = core.science.saturating_add(amount);
        }
    }

    /// Write off beakers: all or nothing (like gold, no "almost enough").
    pub(crate) fn spend_science(&mut self, p: PlayerId, amount: u32) -> bool {
        match self.players.get_mut(p.0 as usize) {
            Some(core) if core.science >= amount => {
                core.science -= amount;
                true
            }
            _ => false,
        }
    }

    /// Join the research queue (checks are in validate/commit).
    pub(crate) fn enqueue_research(&mut self, p: PlayerId, tech: TechId) -> bool {
        match self.players.get_mut(p.0 as usize) {
            Some(core) if core.research_queue.len() < RESEARCH_QUEUE_MAX => {
                core.research_queue.push(tech);
                true
            }
            _ => false,
        }
    }

    /// The caller validates the complete plan before this single write.
    pub(crate) fn replace_research(&mut self, player: PlayerId, queue: Vec<TechId>) -> bool {
        match self.players.get_mut(player.0 as usize) {
            Some(core) if core.alive && queue.len() <= RESEARCH_QUEUE_MAX => { core.research_queue = queue; true }
            _ => false,
        }
    }

    /// Pop the queue head (a completed tech goes to learn_tech).
    pub(crate) fn pop_research(&mut self, p: PlayerId) -> Option<TechId> {
        match self.players.get_mut(p.0 as usize) {
            Some(core) if !core.research_queue.is_empty() => Some(core.research_queue.remove(0)),
            _ => None,
        }
    }

    /// Player era = max era of learned techs (0 without techs). Display only in v1.
    pub fn era_of(&self, p: PlayerId, rules: &crate::rules::CompiledRuleset<M>) -> u8 {
        self.techs_of(p)
            .iter()
            .filter_map(|t| rules.technologies.get(t.0 as usize))
            .map(|d| d.era)
            .max()
            .unwrap_or(0)
    }

    /// Classic era of the player: the latest `EraDefinition::index` among learned techs
    /// (0 = ancient). `era_of` is the depth in the tech graph, not a Classic era.
    pub fn nation_era_of(&self, p: PlayerId, rules: &crate::rules::CompiledRuleset<M>) -> u8 {
        self.techs_of(p)
            .iter()
            .filter_map(|t| rules.technologies.get(t.0 as usize))
            .filter_map(|d| d.content_era)
            .filter_map(|e| rules.eras.get(e.0 as usize))
            .map(|e| e.index)
            .max()
            .unwrap_or(0)
    }

    /// Nation index of the player (rules.nations; 0 = not set / an old save).
    pub fn nation_of(&self, p: PlayerId) -> u8 {
        self.players.get(p.0 as usize).map(|c| c.nation).unwrap_or(0)
    }

    /// Assign a nation (scenario generation/restore; the simulation
    /// does not change nations: it is identity, not a v1 mechanic).
    pub(crate) fn set_nation(&mut self, p: PlayerId, nation: u8) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.nation = nation;
        }
    }

    /// Golden ages, great people counters, science record of a player.
    pub fn economy_of(&self, p: PlayerId) -> Option<&crate::world::module::ModulePlayerOf<M>> {
        self.players.get(p.0 as usize).map(|c| &c.economy)
    }

    pub fn economy_mut(
        &mut self,
        p: PlayerId,
    ) -> Option<&mut crate::world::module::ModulePlayerOf<M>> {
        self.players.get_mut(p.0 as usize).map(|c| &mut c.economy)
    }

    /// Tiles the player has bought.
    #[inline]
    pub fn plots_bought(&self, p: PlayerId) -> u16 {
        self.players.get(p.0 as usize).map_or(0, |c| c.plots_bought)
    }

    pub(crate) fn set_plots_bought(&mut self, p: PlayerId, bought: u16) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.plots_bought = bought;
        }
    }

    /// One more tile bought.
    pub fn count_plot_bought(&mut self, p: PlayerId) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.plots_bought = core.plots_bought.saturating_add(1);
        }
    }

    pub fn kind_of(&self, p: PlayerId) -> PlayerKind<M> {
        self.players
            .get(p.0 as usize)
            .map(|c| c.kind)
            .unwrap_or_default()
    }

    /// A major nation (not a city-state, not barbarians).
    pub fn is_major(&self, p: PlayerId) -> bool {
        self.kind_of(p) == PlayerKind::Major
    }

    /// Set a player's kind (scenario setup and restore only).
    pub(crate) fn set_kind(&mut self, p: PlayerId, kind: PlayerKind<M>) {
        if let Some(core) = self.players.get_mut(p.0 as usize) {
            core.kind = kind;
        }
    }

    /// Restore from the save schema.
    pub(crate) fn from_parts(
        alive: Vec<bool>,
        techs: Vec<Vec<TechId>>,
        gold: Vec<u32>,
        science: Vec<u32>,
        research: Vec<Vec<TechId>>,
        nations: Vec<u8>,
    ) -> Self {
        let mut it = techs.into_iter();
        let mut g = gold.into_iter();
        let mut s = science.into_iter();
        let mut r = research.into_iter();
        let mut c = nations.into_iter();
        Self {
            players: alive
                .into_iter()
                .map(|a| PlayerCore {
                    alive: a,
                    techs: it.next().unwrap_or_default(),
                    treasury: g.next().unwrap_or(0),
                    science: s.next().unwrap_or(0),
                    research_queue: r.next().unwrap_or_default(),
                    nation: c.next().unwrap_or(0),
                    plots_bought: 0,
                    economy: Default::default(),
                    kind: PlayerKind::Major,
                })
                .collect(),
        }
    }
}

/// The player's era: the latest content era of its known technologies,
/// as the era's order (0 before any).
pub fn player_era<M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &crate::rules::CompiledRuleset<M>,
    player: PlayerId,
) -> u8 {
    let mut era = 0;
    for tech in auth.players().techs_of(player) {
        let Some(def) = rules.technologies.get(tech.0 as usize) else {
            continue;
        };
        let Some(content_era) = def.content_era else {
            continue;
        };
        if let Some(e) = rules.eras.get(content_era.0 as usize) {
            era = era.max(e.index);
        }
    }
    era
}
