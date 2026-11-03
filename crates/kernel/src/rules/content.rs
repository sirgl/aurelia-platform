//! Classic content vocabulary shared by the source and compiled rules:
//! ids of the tables added for the full ruleset, yield sets, classification
//! enums and inert effects.
//!
//! Inert effect: a catalog effect (or a unit ability / building requirement)
//! whose behaviour needs a mechanic the engine does not have yet. It is
//! compiled and its references are validated, but nothing evaluates it. Its
//! `needs_mechanic` names the missing mechanic, so the set of inert effects
//! of a ruleset is the work list of M4 (classic/docs/classic/implemented.md).

use crate::module::RulesModule;
use crate::rules::effects::YieldKind;
use crate::rules::source::StableContentKey;

macro_rules! content_ids {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {$(
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(pub u16);
    )*};
}

content_ids!(
    /// Index into `CompiledRuleset::eras` (sorted by key).
    EraId,
    /// Index into `CompiledRuleset::terrains`. Not the map's `TerrainId`
    /// (a map code); `TerrainDefinition::map_code` links the two.
    TerrainKindId,
    NaturalWonderId,
    ProjectId,
    SpecialistId,
);

/// Yields of a tile, feature, improvement, resource or specialist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Yields {
    pub food: i32,
    pub production: i32,
    pub gold: i32,
    pub science: i32,
    pub culture: i32,
    /// The rules module's one yield channel (`Vocabulary::YIELDS`; Classic:
    /// faith).
    pub module: i32,
}

impl Yields {
    /// Nothing in every channel.
    pub const ZERO: Yields = Yields { food: 0, production: 0, gold: 0, science: 0, culture: 0, module: 0 };

    /// Amount of one channel (`Output` is the v1 combined channel: none).
    pub fn of<M: RulesModule>(&self, kind: YieldKind<M>) -> i32 {
        match kind {
            YieldKind::Output => 0,
            YieldKind::Food => self.food,
            YieldKind::Production => self.production,
            YieldKind::Gold => self.gold,
            YieldKind::Science => self.science,
            YieldKind::Culture => self.culture,
            // The module's yield (one at most, `Vocabulary::YIELDS`).
            YieldKind::Module(_) => self.module,
        }
    }

    pub fn is_zero(&self) -> bool {
        *self == Yields::default()
    }

    /// Channel-wise sum.
    pub fn plus(self, other: Yields) -> Yields {
        Yields {
            food: self.food + other.food,
            production: self.production + other.production,
            gold: self.gold + other.gold,
            science: self.science + other.science,
            culture: self.culture + other.culture,
            module: self.module + other.module,
        }
    }

    /// Channel-wise difference.
    pub fn minus(self, other: Yields) -> Yields {
        Yields {
            food: self.food - other.food,
            production: self.production - other.production,
            gold: self.gold - other.gold,
            science: self.science - other.science,
            culture: self.culture - other.culture,
            module: self.module - other.module,
        }
    }

    /// Add `amount` to one channel (`Output`, the legacy combined channel,
    /// lands on production).
    pub fn add<M: RulesModule>(&mut self, kind: YieldKind<M>, amount: i32) {
        match kind {
            YieldKind::Output | YieldKind::Production => self.production += amount,
            YieldKind::Food => self.food += amount,
            YieldKind::Gold => self.gold += amount,
            YieldKind::Science => self.science += amount,
            YieldKind::Culture => self.culture += amount,
            YieldKind::Module(_) => self.module += amount,
        }
    }

    /// Every channel at least that channel of `min`.
    pub fn at_least_each(self, min: Yields) -> Yields {
        Yields {
            food: self.food.max(min.food),
            production: self.production.max(min.production),
            gold: self.gold.max(min.gold),
            science: self.science.max(min.science),
            culture: self.culture.max(min.culture),
            module: self.module.max(min.module),
        }
    }

    /// Every channel at least `min` (Classic clamps tile yields at 0).
    pub fn at_least(self, min: i32) -> Yields {
        Yields {
            food: self.food.max(min),
            production: self.production.max(min),
            gold: self.gold.max(min),
            science: self.science.max(min),
            culture: self.culture.max(min),
            module: self.module.max(min),
        }
    }
}

/// Where an improvement may be built beyond its terrain, feature and
/// resource lists (placement flags).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ImprovementPlacement {
    /// Valid on any hills tile (mine, terrace farm).
    pub hills_makes_valid: bool,
    /// Valid on any tile with fresh water (farm).
    pub fresh_water_makes_valid: bool,
    /// Hills need fresh water (farm).
    pub requires_flatlands_or_fresh_water: bool,
    /// Valid only on one of its features (lumber mill).
    pub requires_feature: bool,
    /// May replace a resource it does not connect (great person
    /// improvements, fort).
    pub buildable_on_resources: bool,
    /// Built on water by a work boat (`Water`).
    pub water: bool,
    /// The tile must touch the sea (`Coastal`: moai, feitoria).
    pub coastal: bool,
    /// Not next to the same improvement (chateau).
    pub no_two_adjacent: bool,
    /// Next to a luxury resource (chateau).
    pub adjacent_luxury: bool,
    /// On own land or next to it (citadel).
    pub in_adjacent_friendly: bool,
    /// Also outside own borders (fort).
    pub outside_borders: bool,
    /// On anybody's land (archaeological dig, landmark).
    pub ignore_ownership: bool,
    /// Only on the land of a player that hosts such improvements
    /// (`ModulePlayerKind::hosts_improvements`: feitoria).
    pub only_host_land: bool,
    /// Only over another improvement, which it replaces
    /// (landmark over a dig).
    pub requires_improvement: bool,
    /// Its builder chooses what to do with the tile when it is done
    /// (archaeological dig).
    pub prompt_when_complete: bool,
    /// Pillaging removes it instead of damaging it
    /// (fishing boats, fort, offshore platform).
    pub destroyed_when_pillaged: bool,
}

impl ImprovementPlacement {
    /// The flags that say where on the map it fits (terrain, features,
    /// water, neighbours), without who may build it and what happens to it.
    pub(crate) fn site_flags(&self) -> ImprovementPlacement {
        ImprovementPlacement {
            ignore_ownership: false,
            only_host_land: false,
            requires_improvement: false,
            prompt_when_complete: false,
            destroyed_when_pillaged: false,
            ..*self
        }
    }
}

/// Where a building may be built (placement flags and local resource
/// requirements), as content writes it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SourceBuildingPlacement {
    /// The city touches a water body of at least `MIN_WATER_SIZE_FOR_OCEAN`
    /// tiles (`Water`: harbor, lighthouse).
    pub water: bool,
    /// The city is on a river (`River`: water mill).
    pub river: bool,
    /// A mountain next to the city (`Mountain`: observatory).
    pub mountain: bool,
    /// A mountain within the city's territory.
    pub nearby_mountain: bool,
    /// The city has fresh water (`FreshWater`: garden).
    pub fresh_water: bool,
    /// The city is not on hills (`Flat`: windmill).
    pub flat: bool,
    /// A tile of this terrain within the city's territory.
    pub nearby_terrain: Option<StableContentKey>,
    /// The city does not stand on this terrain.
    pub prohibited_city_terrain: Option<StableContentKey>,
    /// One of these resources within the city's territory.
    pub local_resources_any: Vec<StableContentKey>,
    /// All of these resources within the city's territory.
    pub local_resources_all: Vec<StableContentKey>,
    /// One building of a group per city.
    pub exclusive_group: Option<u8>,
    /// Only the capital (the palace).
    pub capital: bool,
    /// Forms a city connection over water (harbor).
    pub allows_water_routes: bool,
}

/// A word of the rules module in one of the kernel's content enums
/// (`UnitRole::Module`, `BuildingKind::Module`, ...): its tag, a position
/// the kernel's words leave free in the enum the family was before the
/// split. The enums hash and order by tags, as the derived `Hash` and
/// `Ord` went by the discriminant, so the ruleset fingerprint is unchanged.
macro_rules! module_tag {
    ($(#[$doc:meta])* $name:ident, kernel: $kernel:pat) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(u8);

        impl $name {
            /// The tag `tag`; a kernel word's tag fails the module's build
            /// (its words are constants).
            pub const fn new(tag: u8) -> Self {
                assert!(!matches!(tag, $kernel), "a kernel word's tag");
                $name(tag)
            }
        }
    };
}

module_tag!(
    /// The tag of a module's table (`ContentTable::Module`).
    TableTag, kernel: 0..=7 | 10..=13 | 18 | 20..=22
);
module_tag!(
    /// The tag of a module's unit role (`UnitRole::Module`).
    RoleTag, kernel: 0 | 3..=18
);
module_tag!(
    /// The tag of a module's kind of building (`BuildingKind::Module`).
    BuildingKindTag, kernel: 0 | 3..=6
);
module_tag!(
    /// The tag of a module's kind of improvement (`ImprovementKind::Module`).
    ImprovementKindTag, kernel: 0 | 2 | 4
);

/// Hash and order a content enum by its tag (`module_tag!`).
macro_rules! by_tag {
    ($name:ident) => {
        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                crate::rules::vocabulary::hash_tag(self.tag(), state);
            }
        }

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                self.tag().cmp(&other.tag())
            }
        }
    };
}

/// A unit's role: the unit combat type of military units, the kernel's
/// non-combat roles and the module's (Classic: great people, religious
/// units), which are non-combat roles too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum UnitRole {
    Civilian,
    SpaceshipPart,
    Recon,
    Melee,
    Archer,
    Mounted,
    Siege,
    Gun,
    Armor,
    NavalMelee,
    NavalRanged,
    Submarine,
    Carrier,
    Fighter,
    Bomber,
    Helicopter,
    Missile,
    /// A non-combat role of the rules module.
    #[cfg_attr(feature = "test-fixtures", serde(skip))]
    Module(RoleTag),
}

by_tag!(UnitRole);

impl UnitRole {
    /// Military roles are the unit combat types (promotion classes).
    pub fn is_military(self) -> bool {
        !matches!(self, UnitRole::Civilian | UnitRole::SpaceshipPart | UnitRole::Module(_))
    }

    /// The role's tag (`module_tag!`).
    pub const fn tag(self) -> u8 {
        match self {
            UnitRole::Civilian => 0,
            UnitRole::SpaceshipPart => 3,
            UnitRole::Recon => 4,
            UnitRole::Melee => 5,
            UnitRole::Archer => 6,
            UnitRole::Mounted => 7,
            UnitRole::Siege => 8,
            UnitRole::Gun => 9,
            UnitRole::Armor => 10,
            UnitRole::NavalMelee => 11,
            UnitRole::NavalRanged => 12,
            UnitRole::Submarine => 13,
            UnitRole::Carrier => 14,
            UnitRole::Fighter => 15,
            UnitRole::Bomber => 16,
            UnitRole::Helicopter => 17,
            UnitRole::Missile => 18,
            UnitRole::Module(RoleTag(tag)) => tag,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum Domain {
    #[default]
    Land,
    Sea,
    Air,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildingKind {
    #[default]
    Regular,
    Palace,
    /// Nation unique building (replaces a regular one).
    Unique,
    WorldWonder,
    NationalWonder,
    /// A kind of the rules module (Classic: religious buildings, guilds).
    Module(BuildingKindTag),
}

by_tag!(BuildingKind);

impl BuildingKind {
    /// The kind's tag (`module_tag!`).
    pub const fn tag(self) -> u8 {
        match self {
            BuildingKind::Regular => 0,
            BuildingKind::Palace => 3,
            BuildingKind::Unique => 4,
            BuildingKind::WorldWonder => 5,
            BuildingKind::NationalWonder => 6,
            BuildingKind::Module(BuildingKindTag(tag)) => tag,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "test-fixtures", derive(serde::Deserialize))]
pub enum ResourceClass {
    Bonus,
    Strategic,
    Luxury,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ImprovementKind {
    #[default]
    Regular,
    /// Nation unique improvement.
    Unique,
    AncientRuins,
    /// A kind of the rules module (Classic: great person improvements,
    /// barbarian encampments).
    Module(ImprovementKindTag),
}

by_tag!(ImprovementKind);

impl ImprovementKind {
    /// The kind's tag (`module_tag!`).
    pub const fn tag(self) -> u8 {
        match self {
            ImprovementKind::Regular => 0,
            ImprovementKind::Unique => 2,
            ImprovementKind::AncientRuins => 4,
            ImprovementKind::Module(ImprovementKindTag(tag)) => tag,
        }
    }
}

/// Region of a nation start bias (start bias).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum StartRegion {
    Grass,
    Plains,
    Desert,
    Tundra,
    Hills,
    Forest,
    Jungle,
}

/// Region priorities of a start bias (empty for "no bias").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct StartRegions {
    pub prefer: Vec<StartRegion>,
    pub avoid: Vec<StartRegion>,
    /// Placed first along the ocean (Polynesia).
    pub along_ocean: bool,
}

/// How a worker removes a feature.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceFeatureRemoval {
    /// Technology that allows the removal (None: always allowed).
    pub tech: Option<StableContentKey>,
    /// Worker time in hundredths of worker-turns (also added to an
    /// improvement built over the feature).
    pub work_time: u32,
    /// Production granted to the nearest own city (Classic chop production,
    /// before the distance fall-off).
    pub production: u32,
}

/// Condition of an improvement yield that a technology adds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FreshWater {
    Any,
    Required,
    Absent,
}

/// Amount of an inert effect, as written in the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InertAmount {
    /// No value (text-only behaviour).
    None,
    /// Boolean flag set to true.
    Flag,
    Int(i64),
    /// A fractional value in hundredths (2.5 → 250).
    Hundredths(i64),
}

/// Inert effect as written in content (see the module docs).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceInertEffect {
    /// Name of the missing mechanic (M4 work list key).
    pub needs_mechanic: String,
    /// Catalog modifier name (`combat.attack_vs_wounded.pct`, `unit.ability.pillage`, ...).
    pub modifier: String,
    /// Catalog scope / selector kind / trigger, kept for the mechanic author.
    pub scope: String,
    pub selector: String,
    pub trigger: String,
    pub amount: InertAmount,
    /// Every content key the effect names (target, filters, conditions).
    pub refs: Vec<StableContentKey>,
}

/// Table a validated reference points into: a table of the kernel or one
/// of the rules module's tables (`ModuleTables::source_keys`).
///
/// Every table has a tag: its position in the one closed enum this was
/// before the module's tables left the kernel. `Hash` writes the tag as the
/// derived `Hash` of that enum wrote its discriminant (the ruleset
/// fingerprint hashes the references of inert effects), so the module's
/// tables take the tags the kernel leaves free ([`TableTag`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTable {
    Unit,
    Building,
    Technology,
    Improvement,
    Feature,
    Resource,
    Nation,
    Promotion,
    Era,
    Terrain,
    NaturalWonder,
    /// Projects: the kernel produces them, the module holds their rows
    /// (`ModuleTables::project`).
    Project,
    Specialist,
    /// `core:unit-class/<slug>`: a class some unit declares.
    UnitClass,
    /// `core:building-class/<slug>`: a class some building declares.
    BuildingClass,
    /// Vocabulary without a table (`core:unit-combat/`, `core:domain/`,
    /// `core:route/`, `core:build/`, `core:great-work-slot/`, ...): the key
    /// is checked against the fixed vocabulary, `index` is 0.
    Vocabulary,
    /// A table of the rules module.
    Module(TableTag),
}

impl ContentTable {
    /// The table's tag (see the type's docs).
    pub const fn tag(self) -> u8 {
        match self {
            ContentTable::Unit => 0,
            ContentTable::Building => 1,
            ContentTable::Technology => 2,
            ContentTable::Improvement => 3,
            ContentTable::Feature => 4,
            ContentTable::Resource => 5,
            ContentTable::Nation => 6,
            ContentTable::Promotion => 7,
            ContentTable::Era => 10,
            ContentTable::Terrain => 11,
            ContentTable::NaturalWonder => 12,
            ContentTable::Project => 13,
            ContentTable::Specialist => 18,
            ContentTable::UnitClass => 20,
            ContentTable::BuildingClass => 21,
            ContentTable::Vocabulary => 22,
            ContentTable::Module(TableTag(tag)) => tag,
        }
    }
}

impl std::hash::Hash for ContentTable {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        crate::rules::vocabulary::hash_tag(self.tag(), state);
    }
}

/// A resolved reference of an inert effect.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContentRef {
    pub table: ContentTable,
    pub index: u16,
    pub key: String,
}

/// Compiled inert effect: references resolved, nothing evaluates it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InertEffect {
    pub needs_mechanic: String,
    pub modifier: String,
    pub scope: String,
    pub selector: String,
    pub trigger: String,
    pub amount: InertAmount,
    pub refs: Vec<ContentRef>,
}

/// What an aircraft carrier or a missile carrier holds and what an air unit counts as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CargoKind {
    Aircraft,
    Missile,
}

/// Combat abilities of a unit kind as written in content (the fields the combat,
/// air and nuclear systems read).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct SourceUnitCombat {
    /// Becomes this unit when captured (settler → worker).
    pub capture: Option<crate::rules::source::StableContentKey>,
    /// Ignores the defense of city buildings.
    pub ignore_building_defense: bool,
    /// Cannot move by itself (aircraft: rebase only).
    pub immobile: bool,
    /// Interception range.
    pub air_intercept_range: u8,
    /// Destroyed by its own attack (missiles, `Suicide`).
    pub suicide: bool,
    /// Nuclear damage level (1 atomic bomb, 2 missile).
    pub nuke_level: u8,
    /// Attacks only units of its own domain (submarines).
    pub ranged_only_in_domain: bool,
    /// What it carries (carrier, missile cruiser, nuclear submarine).
    pub carries: Option<CargoKind>,
    /// What it is as cargo (fighter-type aircraft, missiles).
    pub carried_as: Option<CargoKind>,
    /// May pillage improvements and roads.
    pub pillage: bool,
}

/// Compiled combat abilities of a unit kind (references resolved).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UnitCombatTraits {
    pub capture: Option<crate::ids::UnitKindId>,
    pub ignore_building_defense: bool,
    pub immobile: bool,
    pub air_intercept_range: u8,
    pub suicide: bool,
    pub nuke_level: u8,
    pub ranged_only_in_domain: bool,
    pub carries: Option<CargoKind>,
    pub carried_as: Option<CargoKind>,
    pub pillage: bool,
}
