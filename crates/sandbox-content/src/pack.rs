//! The schema of the sandbox pack and its translation into the kernel's
//! source rules. The pack writes only what the sandbox plays with; every
//! other field of the kernel's rows keeps its default ("not declared").

use kernel::rules::content::{BuildingKind, UnitRole, Yields};
use kernel::rules::effects::{Scope, Selector, YieldKind};
use kernel::rules::roles::{FeatureRole, Ground, Climate, StartingUnit};
use kernel::rules::source::{
    KeyCount, SourceBuilding, SourceNation, SourceEffect, SourceEra, SourceFeature, SourceModifier, SourceRules, SourceTerrain,
    SourceUnit, StableContentKey,
};
use sandbox_rules::Sandbox;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Pack {
    pub terrains: Vec<Terrain>,
    pub grove: Feature,
    pub units: Vec<Unit>,
    pub buildings: Vec<Building>,
    pub factions: Vec<Faction>,
    pub start: Start,
    /// Colours of the renderer's ground texture layers, in its order.
    pub layers: Vec<(u8, u8, u8)>,
}

/// The ground a terrain plays for the map generator.
#[derive(Debug, Clone, Copy, Deserialize)]
pub enum PackGround {
    Water,
    Mountains,
    Hills,
    Humid,
    Temperate,
    Arid,
    Cold,
    Frozen,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Terrain {
    pub key: String,
    pub name: String,
    pub ground: PackGround,
    pub code: u16,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
    #[serde(default = "one")]
    pub moves: u8,
    #[serde(default)]
    pub defence: i32,
    #[serde(default)]
    pub impassable: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Feature {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub production: i32,
    #[serde(default = "one")]
    pub moves: u8,
    #[serde(default)]
    pub defence: i32,
}

/// What a unit is in the game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum UnitKind {
    /// Founds cities, never fights.
    Settler,
    Scout,
    Soldier,
    Rider,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Unit {
    pub key: String,
    pub name: String,
    pub kind: UnitKind,
    pub cost: u32,
    pub moves: u16,
    #[serde(default)]
    pub strength: u16,
    /// The model the renderer draws (an asset id of its manifest).
    pub model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Building {
    pub key: String,
    pub name: String,
    /// The seat of government: every first city gets it, it is not built.
    #[serde(default)]
    pub seat: bool,
    #[serde(default)]
    pub cost: u32,
    #[serde(default)]
    pub food: i32,
    #[serde(default)]
    pub production: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Faction {
    pub key: String,
    pub name: String,
    pub leader: String,
    pub color: (u8, u8, u8),
    pub cities: Vec<String>,
}

/// What every faction starts with.
#[derive(Debug, Clone, Deserialize)]
pub struct Start {
    pub era: String,
    pub settlers: u16,
    pub defender: String,
    pub explorer: String,
}

fn one() -> u8 {
    1
}

fn key(key: &str) -> StableContentKey {
    StableContentKey::new(key)
}

impl Pack {
    /// The kernel's source rules of the pack, roles included.
    pub fn source_rules(&self) -> SourceRules<Sandbox> {
        let mut rules = SourceRules::<Sandbox> {
            terrains: self.terrains.iter().map(Terrain::source).collect(),
            features: vec![self.grove.source()],
            units: self.units.iter().map(Unit::source).collect(),
            buildings: self.buildings.iter().map(Building::source).collect(),
            nations: self.factions.iter().map(|f| f.source(&self.settler_key(), self.start.settlers)).collect(),
            eras: vec![self.start.era()],
            ..SourceRules::default()
        };
        let roles = &mut rules.roles;
        roles.grounds = self.terrains.iter().map(|t| (t.ground.kernel(), key(&t.key))).collect();
        roles.features = vec![(FeatureRole::Forest, key(&self.grove.key))];
        roles.starting_units = vec![
            (StartingUnit::Defense, key(&self.start.defender)),
            (StartingUnit::Explore, key(&self.start.explorer)),
        ];
        rules
    }

    fn settler_key(&self) -> String {
        let settler = self.units.iter().find(|u| u.kind == UnitKind::Settler);
        settler.map(|u| u.key.clone()).expect("the pack has a settler")
    }
}

impl PackGround {
    fn kernel(self) -> Ground {
        match self {
            PackGround::Water => Ground::Water,
            PackGround::Mountains => Ground::Mountains,
            PackGround::Hills => Ground::Hills,
            PackGround::Humid => Ground::Flat(Climate::Humid),
            PackGround::Temperate => Ground::Flat(Climate::Temperate),
            PackGround::Arid => Ground::Flat(Climate::Arid),
            PackGround::Cold => Ground::Flat(Climate::Cold),
            PackGround::Frozen => Ground::Flat(Climate::Frozen),
        }
    }
}

impl Terrain {
    fn source(&self) -> SourceTerrain {
        let water = matches!(self.ground, PackGround::Water);
        SourceTerrain {
            id: Some(key(&self.key)),
            name: self.name.clone(),
            yields: Yields { food: self.food, production: self.production, ..Yields::default() },
            movement_cost: self.moves,
            impassable: self.impassable,
            defense_pct: self.defence,
            water,
            can_found_city: !water && !self.impassable,
            map_code: Some(self.code),
            ..SourceTerrain::default()
        }
    }
}

impl Feature {
    fn source(&self) -> SourceFeature {
        SourceFeature {
            id: Some(key(&self.key)),
            name: self.name.clone(),
            yields: Yields { production: self.production, ..Yields::default() },
            movement_cost: self.moves,
            defense_pct: self.defence,
            rough: true,
            // No workers in the sandbox: nothing removes a grove.
            unremovable: true,
            ..SourceFeature::default()
        }
    }
}

impl Unit {
    fn source(&self) -> SourceUnit<Sandbox> {
        let role = match self.kind {
            UnitKind::Settler => UnitRole::Civilian,
            UnitKind::Scout => UnitRole::Recon,
            UnitKind::Soldier => UnitRole::Melee,
            UnitKind::Rider => UnitRole::Mounted,
        };
        SourceUnit {
            id: Some(key(&self.key)),
            name: self.name.clone(),
            movement: self.moves,
            strength: self.strength,
            founder: self.kind == UnitKind::Settler,
            cost: self.cost,
            role: Some(role),
            ..SourceUnit::default()
        }
    }
}

impl Building {
    fn source(&self) -> SourceBuilding<Sandbox> {
        let add = |yield_kind, amount| {
            SourceEffect::passive(Scope::City, Selector::SourceCity, SourceModifier::AddYield { yield_kind, amount })
        };
        let mut effects = Vec::new();
        if self.food != 0 {
            effects.push(add(YieldKind::Food, self.food));
        }
        if self.production != 0 {
            effects.push(add(YieldKind::Production, self.production));
        }
        SourceBuilding {
            id: Some(key(&self.key)),
            name: self.name.clone(),
            cost: self.cost,
            kind: if self.seat { BuildingKind::Palace } else { BuildingKind::Regular },
            unbuildable: self.seat,
            effects,
            ..SourceBuilding::default()
        }
    }
}

impl Faction {
    fn source(&self, settler: &str, settlers: u16) -> SourceNation<Sandbox> {
        SourceNation {
            id: Some(key(&self.key)),
            name: self.name.clone(),
            leader: self.leader.clone(),
            color: self.color,
            adjective: self.name.clone(),
            free_units: vec![KeyCount { key: key(settler), count: settlers }],
            city_names: self.cities.clone(),
            ..SourceNation::default()
        }
    }
}

impl Start {
    /// The one era: one unit of each starting role.
    fn era(&self) -> SourceEra<Sandbox> {
        SourceEra {
            id: Some(key(&self.era)),
            name: "Dawn".into(),
            starting_defense_units: 1,
            starting_explore_units: 1,
            ..SourceEra::default()
        }
    }
}
