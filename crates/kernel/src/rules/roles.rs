//! Roles: the parts of a game the kernel names, bound to content.
//!
//! The kernel never names a content key. Where a rule needs a particular
//! thing (the hills a unit climbs, the water the map generator lays) it
//! asks for a role, and the ruleset binds every role to one of its rows
//! (`roles` in the content). A binding names one row, so a role has at most
//! one carrier by construction; the compiler checks that the carrier fits
//! the role and that the roles the kernel cannot do without are bound.
//!
//! Roles live beside the rows, not in them: the fingerprint of rules
//! written before roles existed stays reproducible (`fingerprint`), so
//! saves of that time keep loading.

use crate::rules::tables::ModuleTables;
use crate::module::RulesModule;
use crate::ids::{FeatureId, TechId, UnitKindId};
use crate::rules::compile::RulesError;
use crate::rules::compiled::{CompiledRuleset, HandicapDefinition, TerrainDefinition};
use crate::rules::content::{ContentTable, SpecialistId, TerrainKindId};
use crate::rules::source::StableContentKey;

/// The climate of a flat land terrain: where the map generator lays it
/// (latitude and rainfall) and what start placement reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Climate {
    /// Wet temperate land (Classic grassland).
    Humid,
    /// Drier temperate land (Classic plains).
    Temperate,
    /// The dry subtropical belt (Classic desert).
    Arid,
    /// Near the poles (Classic tundra).
    Cold,
    /// At the poles (Classic snow).
    Frozen,
}

impl Climate {
    pub const ALL: [Climate; 5] = [
        Climate::Humid,
        Climate::Temperate,
        Climate::Arid,
        Climate::Cold,
        Climate::Frozen,
    ];
}

/// What a map terrain code is to the kernel. Movement, sight, combat and
/// the map generator read it; every terrain with a map code plays exactly
/// one ground.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Ground {
    /// The one water code of the map (shallow and deep water alike).
    Water,
    Mountains,
    Hills,
    Flat(Climate),
}

impl Ground {
    pub const ALL: [Ground; 8] = [
        Ground::Water,
        Ground::Mountains,
        Ground::Hills,
        Ground::Flat(Climate::Humid),
        Ground::Flat(Climate::Temperate),
        Ground::Flat(Climate::Arid),
        Ground::Flat(Climate::Cold),
        Ground::Flat(Climate::Frozen),
    ];

    /// Position in `ALL`.
    pub const fn index(self) -> usize {
        match self {
            Ground::Water => 0,
            Ground::Mountains => 1,
            Ground::Hills => 2,
            Ground::Flat(climate) => 3 + climate as usize,
        }
    }
}

/// What a feature is to the kernel: what the map generator plants, what
/// trade paths, archaeology and start placement read, what a nuclear blast
/// leaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FeatureRole {
    Forest,
    Jungle,
    Marsh,
    Oasis,
    FloodPlains,
    Ice,
    Atoll,
    Fallout,
}

impl FeatureRole {
    pub const ALL: [FeatureRole; 8] = [
        FeatureRole::Forest,
        FeatureRole::Jungle,
        FeatureRole::Marsh,
        FeatureRole::Oasis,
        FeatureRole::FloodPlains,
        FeatureRole::Ice,
        FeatureRole::Atoll,
        FeatureRole::Fallout,
    ];
}

/// The units a player starts with beside its founders: the era says how
/// many (`starting_defense_units`, `_worker_units`, `_explore_units`), the
/// role says which. The map generator and city-states start with the
/// defense unit too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StartingUnit {
    Defense,
    Worker,
    Explore,
}

/// A production process a technology unlocks (`world::city_economy`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProcessRole {
    Wealth,
    Research,
}

/// The map code of every ground: what the map generator writes and what
/// the kernel reads back from a code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TerrainPalette {
    /// By `Ground::index`.
    codes: [u16; 8],
}

impl TerrainPalette {
    /// The codes of rules without a terrain table (test fixtures paint
    /// them): plains 0, grassland 1, hills 2, desert 3, water 4, snow 5,
    /// mountains 6, tundra 7.
    pub const LEGACY: TerrainPalette = TerrainPalette { codes: [4, 6, 2, 1, 0, 3, 7, 5] };

    pub const fn code(self, ground: Ground) -> u16 {
        self.codes[ground.index()]
    }

    /// The ground of a map code (None: not a code of the palette).
    pub fn ground(self, code: u16) -> Option<Ground> {
        Ground::ALL.into_iter().find(|g| self.code(*g) == code)
    }
}

/// Role bindings as the content writes them: each binding names one row.
#[derive(Debug, Clone, Default)]
pub struct SourceRoles<M: RulesModule> {
    /// The terrain playing each ground (all eight when the ruleset has a
    /// terrain table).
    pub grounds: Vec<(Ground, StableContentKey)>,
    /// Water away from land (Classic ocean): the map has no code for it,
    /// combat reads its row for a water tile with no land around.
    pub deep_water: Option<StableContentKey>,
    /// The feature playing each feature role (any may be left out: the
    /// generator does not plant it, nothing reads it).
    pub features: Vec<(FeatureRole, StableContentKey)>,
    pub starting_units: Vec<(StartingUnit, StableContentKey)>,
    /// The vocabulary key a technology unlocks each process with
    /// (`unlocks_actions`).
    pub processes: Vec<(ProcessRole, StableContentKey)>,
    /// The vocabulary key a technology unlocks roads with (unbound: roads
    /// need no technology).
    pub road: Option<StableContentKey>,
    /// The specialist a citizen is when it works nothing (the default specialist);
    /// bound when the ruleset has specialists.
    pub default_specialist: Option<StableContentKey>,
    /// The level a game plays at unless it chooses one (the default difficulty), and
    /// the handicap every AI player plays with; both bound when the ruleset has handicaps.
    pub default_difficulty: Option<StableContentKey>,
    pub ai_handicap: Option<StableContentKey>,
    /// The module's role bindings (`ModuleTables::SourceRoles`).
    pub module: ModuleSourceRoles<M>,
}

/// The module's role bindings as the content writes them.
pub type ModuleSourceRoles<M> = <<M as RulesModule>::Tables as ModuleTables<M>>::SourceRoles;

/// The module's role bindings resolved to rows.
pub type ModuleRoles<M> = <<M as RulesModule>::Tables as ModuleTables<M>>::Roles;

impl<M: RulesModule> SourceRoles<M> {
    /// Take the bindings `other` writes (a ruleset split over files).
    pub fn merge(&mut self, other: SourceRoles<M>) {
        self.grounds.extend(other.grounds);
        self.deep_water = other.deep_water.or(self.deep_water.take());
        self.features.extend(other.features);
        self.starting_units.extend(other.starting_units);
        self.processes.extend(other.processes);
        self.road = other.road.or(self.road.take());
        self.default_specialist = other.default_specialist.or(self.default_specialist.take());
        self.default_difficulty = other.default_difficulty.or(self.default_difficulty.take());
        self.ai_handicap = other.ai_handicap.or(self.ai_handicap.take());
        M::Tables::merge_roles(&mut self.module, other.module);
    }
}

/// Role bindings resolved to rows. Ids follow key order, so the bindings
/// hash the same whatever the source order (`fingerprint`); the module's
/// bindings hash last, where they always were.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Roles<M: RulesModule> {
    /// By `Ground::index`.
    grounds: [Option<TerrainKindId>; 8],
    pub deep_water: Option<TerrainKindId>,
    /// By `FeatureRole as usize`.
    features: [Option<FeatureId>; 8],
    /// By `StartingUnit as usize`.
    starting_units: [Option<UnitKindId>; 3],
    /// The technology unlocking each process, by `ProcessRole as usize`
    /// (None: no technology does, the process stays locked).
    process_techs: [Option<TechId>; 2],
    /// The technology that allows roads (None: roads need none).
    pub road_tech: Option<TechId>,
    pub default_specialist: Option<SpecialistId>,
    /// Positions in `CompiledRuleset::handicaps`.
    default_difficulty: Option<usize>,
    ai_handicap: Option<usize>,
    /// The module's bindings (`ModuleTables::Roles`).
    pub module: ModuleRoles<M>,
}

impl<M: RulesModule> Roles<M> {
    /// The regular unit (before civilization variants) playing `role`.
    pub fn starting_unit(&self, role: StartingUnit) -> Option<UnitKindId> {
        self.starting_units[role as usize]
    }

    /// The technology that unlocks a process.
    pub fn process_tech(&self, role: ProcessRole) -> Option<TechId> {
        self.process_techs[role as usize]
    }

    /// The default game level and the AI's handicap.
    pub fn default_difficulty<'r>(&self, rules: &'r CompiledRuleset<M>) -> Option<&'r HandicapDefinition<M>> {
        rules.handicaps.get(self.default_difficulty?)
    }

    pub fn ai_handicap<'r>(&self, rules: &'r CompiledRuleset<M>) -> Option<&'r HandicapDefinition<M>> {
        rules.handicaps.get(self.ai_handicap?)
    }

    /// The feature playing `role`.
    pub fn feature(&self, role: FeatureRole) -> Option<FeatureId> {
        self.features[role as usize]
    }

    /// The role a feature plays.
    pub fn feature_role(&self, feature: FeatureId) -> Option<FeatureRole> {
        FeatureRole::ALL.into_iter().find(|r| self.feature(*r) == Some(feature))
    }

    /// The terrain playing `ground`.
    pub fn ground(&self, ground: Ground) -> Option<TerrainKindId> {
        self.grounds[ground.index()]
    }

    /// The ground a terrain plays.
    pub fn ground_of(&self, terrain: TerrainKindId) -> Option<Ground> {
        Ground::ALL.into_iter().find(|g| self.ground(*g) == Some(terrain))
    }

    /// The map codes of the grounds; `LEGACY` without a terrain table.
    pub fn palette(&self, rules: &CompiledRuleset<M>) -> TerrainPalette {
        let mut codes = TerrainPalette::LEGACY.codes;
        for ground in Ground::ALL {
            let code = self.ground(ground).and_then(|t| rules.terrains[t.0 as usize].map_code);
            if let Some(code) = code {
                codes[ground.index()] = code;
            }
        }
        TerrainPalette { codes }
    }
}

/// Resolve and check the bindings of `source` against the compiled rows.
pub(crate) fn compile_roles<M: RulesModule>(
    source: &SourceRoles<M>,
    ix: &crate::rules::key_index::KeyIndex,
    rules: &CompiledRuleset<M>,
) -> Result<Roles<M>, RulesError> {
    let mut roles = Roles::default();
    for (ground, key) in &source.grounds {
        let terrain = TerrainKindId(ix.id(ContentTable::Terrain, &key.0)?);
        bind(&mut roles.grounds[ground.index()], terrain, &format!("{ground:?}"))?;
    }
    roles.deep_water = match &source.deep_water {
        Some(key) => Some(TerrainKindId(ix.id(ContentTable::Terrain, &key.0)?)),
        None => None,
    };
    for (role, key) in &source.features {
        let feature = FeatureId(ix.id(ContentTable::Feature, &key.0)?);
        if roles.features.contains(&Some(feature)) {
            return Err(RulesError::Inconsistent(format!("{} plays two feature roles", key.0)));
        }
        bind(&mut roles.features[*role as usize], feature, &format!("{role:?}"))?;
    }
    for (role, key) in &source.starting_units {
        let unit = UnitKindId(ix.id(ContentTable::Unit, &key.0)?);
        bind(&mut roles.starting_units[*role as usize], unit, &format!("{role:?}"))?;
    }
    for (role, key) in &source.processes {
        let tech = unlocking_tech(rules, key)?;
        bind(&mut roles.process_techs[*role as usize], tech, &format!("{role:?}"))?;
    }
    if let Some(key) = &source.road {
        roles.road_tech = Some(unlocking_tech(rules, key)?);
    }
    roles.default_specialist = match &source.default_specialist {
        Some(key) => Some(SpecialistId(ix.id(ContentTable::Specialist, &key.0)?)),
        None => None,
    };
    roles.default_difficulty = handicap(rules, &source.default_difficulty)?;
    roles.ai_handicap = handicap(rules, &source.ai_handicap)?;
    roles.module = M::Tables::compile_roles(&source.module, ix, rules)?;
    check_terrains(&roles, rules)?;
    check_required(&roles, rules)?;
    Ok(roles)
}

/// The first technology (in id order) whose `unlocks_actions` lists `key`.
fn unlocking_tech<M: RulesModule>(rules: &CompiledRuleset<M>, key: &StableContentKey) -> Result<TechId, RulesError> {
    let at = rules.technologies.iter().position(|t| t.unlocks_actions.contains(&key.0));
    at.map(|i| TechId(i as u16))
        .ok_or_else(|| RulesError::Inconsistent(format!("no technology unlocks {}", key.0)))
}

/// Position of a handicap by key.
fn handicap<M: RulesModule>(rules: &CompiledRuleset<M>, key: &Option<StableContentKey>) -> Result<Option<usize>, RulesError> {
    let Some(key) = key else {
        return Ok(None);
    };
    match rules.handicaps.iter().position(|h| h.key == key.0) {
        Some(at) => Ok(Some(at)),
        None => Err(RulesError::UnknownReference(key.0.clone())),
    }
}

/// A ruleset with specialists names its default one; with handicaps, its
/// default level and the AI's handicap.
fn check_required<M: RulesModule>(roles: &Roles<M>, rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    let missing = |role: &str| Err(RulesError::Inconsistent(format!("role {role} has no carrier")));
    if !rules.specialists.is_empty() && roles.default_specialist.is_none() {
        return missing("default_specialist");
    }
    if !rules.handicaps.is_empty() && roles.default_difficulty.is_none() {
        return missing("default_difficulty");
    }
    if !rules.handicaps.is_empty() && roles.ai_handicap.is_none() {
        return missing("ai_handicap");
    }
    Ok(())
}

/// Bind `slot` once: a second carrier of the same role is an error.
pub fn bind<T>(slot: &mut Option<T>, value: T, role: &str) -> Result<(), RulesError> {
    if slot.is_some() {
        return Err(RulesError::Inconsistent(format!("role {role} bound twice")));
    }
    *slot = Some(value);
    Ok(())
}

/// A ruleset with terrains binds every ground to its own map-coded terrain
/// (water to a water terrain, the rest to land) and every map code to a
/// ground; deep water, when bound, is water.
fn check_terrains<M: RulesModule>(roles: &Roles<M>, rules: &CompiledRuleset<M>) -> Result<(), RulesError> {
    if rules.terrains.is_empty() {
        return Ok(());
    }
    let def = |t: TerrainKindId| -> &TerrainDefinition { &rules.terrains[t.0 as usize] };
    let mut seen = Vec::with_capacity(Ground::ALL.len());
    for ground in Ground::ALL {
        let terrain = roles
            .ground(ground)
            .ok_or_else(|| RulesError::Inconsistent(format!("role {ground:?} has no terrain")))?;
        let row = def(terrain);
        if row.map_code.is_none() || row.water != (ground == Ground::Water) || seen.contains(&terrain) {
            return Err(RulesError::Inconsistent(format!("{} cannot play {ground:?}", row.key)));
        }
        seen.push(terrain);
    }
    let unbound = rules.terrains.iter().enumerate().find(|(i, row)| {
        row.map_code.is_some() && roles.ground_of(TerrainKindId(*i as u16)).is_none()
    });
    if let Some((_, row)) = unbound {
        return Err(RulesError::Inconsistent(format!("{} has a map code but plays no ground", row.key)));
    }
    if let Some(deep) = roles.deep_water.filter(|t| !def(*t).water) {
        return Err(RulesError::Inconsistent(format!("{} cannot play deep water", def(deep).key)));
    }
    Ok(())
}

