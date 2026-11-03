//! ChangeJournal: what changed, not what to recompute (§14).
//! Forbidden: InvalidateVisibility/InvalidateRenderer.

use crate::module::RulesModule;
use crate::ids::{CityHandle, PlayerId, TileId, UnitHandle};

#[derive(Debug, Clone, Default)]
pub struct ChangeJournal<M: RulesModule> {
    pub unit_changes: Vec<UnitChange>,
    pub city_changes: Vec<CityChange>,
    pub tile_changes: Vec<TileChange>,
    pub player_changes: Vec<PlayerChange>,
    /// Changes of the rules module's state (`world::module::ModuleChange`).
    pub module_changes: Vec<crate::world::module::ModuleChangeOf<M>>,
}

impl<M: RulesModule> ChangeJournal<M> {
    /// How many changes the journal holds: it only grows, so equal values
    /// of one journal mean that nothing was recorded in between.
    pub fn revision(&self) -> usize {
        self.unit_changes.len()
            + self.city_changes.len()
            + self.tile_changes.len()
            + self.player_changes.len()
            + self.module_changes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.unit_changes.is_empty()
            && self.city_changes.is_empty()
            && self.tile_changes.is_empty()
            && self.player_changes.is_empty()
            && self.module_changes.is_empty()
    }
}

#[derive(Debug, Clone)]
pub enum UnitChange {
    Created(UnitHandle),
    Moved {
        unit: UnitHandle,
        from: TileId,
        to: TileId,
    },
    Damaged {
        unit: UnitHandle,
        damage: u16,
    },
    /// owner/tile at the moment of death, so incremental systems subtract
    /// the delta without a fallback rebuild (a rare LargeWarFront path, but an honest one).
    Destroyed {
        unit: UnitHandle,
        /// Persistent identity: the handle is stale once the unit is gone,
        /// so readers of the journal (the client's presentation) key by it.
        id: crate::ids::EntityId,
        owner: PlayerId,
        tile: TileId,
        kind: crate::ids::UnitKindId,
        /// Its promotions at death: sight depends on them.
        promotions: crate::world::unit::PromotionSet,
    },
    /// The unit took a stance (fortified): no derived system reads it; the
    /// observer's picture shows it.
    StanceChanged { unit: UnitHandle },
    /// Promotion gained (AddPromotion): changes combat/movement modifiers,
    /// no derived system stores them.
    Promoted {
        unit: UnitHandle,
        promotion: crate::ids::PromotionId,
    },
}

#[derive(Debug, Clone)]
pub enum CityChange {
    Founded(CityHandle),
    OwnerChanged {
        city: CityHandle,
        from: PlayerId,
        to: PlayerId,
    },
    BuildingAdded {
        city: CityHandle,
        building: crate::ids::BuildingKindId,
    },
    /// +1 citizen (affects yields through the pop multiplier -> recompute).
    PopulationChanged {
        city: CityHandle,
    },
    /// Manual tile on/off (changes the worked set -> yields recompute;
    /// coverage and explored are untouched, the manual toggle is a no-op there).
    WorkedChanged {
        city: CityHandle,
    },
    /// Conquest status changed (annexed, puppet, razing, liberated), or the
    /// city's capital connection changed: its yields may differ.
    StatusChanged {
        city: CityHandle,
    },
    /// The city was razed and left the world (owner and tile at removal,
    /// like UnitChange::Destroyed).
    Removed {
        city: CityHandle,
        owner: PlayerId,
        tile: TileId,
    },
    /// The head of the city's production queue changed (its production
    /// percents toward the item change, `effects::production`).
    QueueChanged {
        city: CityHandle,
    },
    /// The player gave the city another name from its list: no yield
    /// changes, the map label and the banner show it.
    Renamed {
        city: CityHandle,
    },
}

#[derive(Debug, Clone)]
pub enum TileChange {
    OwnerChanged {
        tile: TileId,
        from: Option<PlayerId>,
        to: Option<PlayerId>,
    },
    /// Improvement built (affects yields through tile bonuses ->
    /// the same recompute path as OwnerChanged; coverage is untouched).
    ImprovementBuilt { tile: TileId },
    /// Resource placed/removed by the generator and debug (affects yields;
    /// coverage is untouched).
    ResourceSet { tile: TileId },
    /// Road built (affects movement, not yields/coverage).
    RoadBuilt { tile: TileId },
    /// River placed by the generator and debug (affects yields through farms
    /// and movement; coverage is untouched).
    RiverSet { tile: TileId },
    /// Feature (forest) placed/removed by the generator and debug (affects yields
    /// through the tile bonus; coverage is untouched).
    FeatureSet { tile: TileId },
    /// Feature (forest) chopped by a worker (the same recompute path as
    /// ImprovementBuilt; coverage is untouched).
    FeatureChopped { tile: TileId },
    /// Natural wonder placed by the generator or debug (the tile's look and, once the
    /// natural wonder mechanic exists, its yields).
    NaturalWonderSet { tile: TileId },
    /// Terrain set by the generator or the harness: yields and the sight
    /// lines (LOS blockers) change. The only tile change visibility reads.
    TerrainSet { tile: TileId },
}

#[derive(Debug, Clone)]
pub enum PlayerChange {
    TechnologyChosen {
        player: PlayerId,
    },
    TurnEnded {
        player: PlayerId,
    },
    WarDeclared {
        player: PlayerId,
        enemy: PlayerId,
    },
    PeaceMade {
        player: PlayerId,
        enemy: PlayerId,
    },
    BordersOpened {
        host: PlayerId,
        guest: PlayerId,
    },
    /// Timed effects of the player started or expired (yields of its cities
    /// may change).
    EffectsChanged {
        player: PlayerId,
    },
    /// The player learned a technology that reveals the map:
    /// it remembers every tile as it is now (`knowledge::sync`).
    MapRevealed {
        player: PlayerId,
    },
    /// The player learned the map within `radius` of `tile` (a rules
    /// module's find, such as a ruin's map): it remembers those tiles as
    /// they are now (`knowledge::sync`).
    AreaRevealed {
        player: PlayerId,
        tile: TileId,
        radius: u8,
    },
    /// The player remembers `tile` as it is now, if it explored it (a
    /// barbarian camp appeared there and the player sees new camps).
    TileNoticed {
        player: PlayerId,
        tile: TileId,
    },
}

/// One command that changed the world and what it changed, in the order
/// applied: the command journal the client's presentation reads (sounds,
/// animations, map feedback). Never read back by the simulation.
#[derive(Debug, Clone)]
pub struct AppliedCommand<M: RulesModule> {
    pub command: M::Command,
    pub changes: ChangeJournal<M>,
}

/// Result of a committed command; downstream (derived/knowledge/client)
/// sees an already consistent world.
#[derive(Debug, Clone)]
pub struct CommandResult<M: RulesModule> {
    journal: ChangeJournal<M>,
}

impl<M: RulesModule> CommandResult<M> {
    pub fn new(journal: ChangeJournal<M>) -> Self {
        Self { journal }
    }

    pub fn changes(&self) -> &ChangeJournal<M> {
        &self.journal
    }

    pub fn into_journal(self) -> ChangeJournal<M> {
        self.journal
    }
}

/// Give `tile` to `city` (owner = the city's owner) and journal the change.
/// Already the city's tile — no-op (no journal entry, derived stays asleep).
fn assign_tile<M: RulesModule>(
    world: &mut crate::world::state::AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
    owner: PlayerId,
    tile: TileId,
) {
    let prev = world.map.owner(tile).unwrap_or(None);
    if prev == Some(owner) && world.map.owning_city(tile) == Some(city) {
        return;
    }
    world.map.set_owner(tile, Some(owner), Some(city));
    journal.tile_changes.push(TileChange::OwnerChanged {
        tile,
        from: prev,
        to: Some(owner),
    });
}

/// One tile for one city (culture growth and purchase — Classic grows
/// borders one plot at a time).
pub fn claim_tile<M: RulesModule>(
    world: &mut crate::world::state::AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
    tile: TileId,
) {
    let Some(owner) = world.cities.get(city).map(|c| c.owner) else {
        return;
    };
    assign_tile(world, journal, city, owner, tile);
}

/// The plots a new city takes (Classic): its own tile, and the ring tiles
/// nobody owns yet — founding never takes another player's or another
/// city's tiles. One path for commit found_city and debug_found_city.
pub(crate) fn claim_founding_ring<M: RulesModule>(
    world: &mut crate::world::state::AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
    radius: i32,
) {
    let Some((owner, center)) = world.cities.get(city).map(|c| (c.owner, c.tile)) else {
        return;
    };
    assign_tile(world, journal, city, owner, center);
    let dims = world.map.dimensions;
    for tile in crate::math::tiles_in_hex_radius(&dims, center, radius) {
        if world.map.owner(tile) == Some(None) {
            assign_tile(world, journal, city, owner, tile);
        }
    }
}

/// A razed city's plots become unowned (journal per tile).
pub fn release_city_tiles<M: RulesModule>(
    world: &mut crate::world::state::AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
) {
    let Some(center) = world.cities.get(city).map(|c| c.tile) else {
        return;
    };
    let dims = world.map.dimensions;
    let radius = crate::world::city::CULTURE_RADIUS_MAX as i32;
    for tile in crate::math::tiles_in_hex_radius(&dims, center, radius) {
        if world.map.owning_city(tile) == Some(city) {
            let prev = world.map.owner(tile).unwrap_or(None);
            world.map.set_owner(tile, None, None);
            journal.tile_changes.push(TileChange::OwnerChanged {
                tile,
                from: prev,
                to: None,
            });
        }
    }
}

/// A captured city keeps its plots: every tile that belongs to `city` gets
/// the city's new owner. Tiles of other cities and players stay as they
/// are. A city's tiles lie within CULTURE_RADIUS_MAX of its center.
pub fn transfer_city_tiles<M: RulesModule>(
    world: &mut crate::world::state::AuthWorld<M>,
    journal: &mut ChangeJournal<M>,
    city: CityHandle,
) {
    let Some((owner, center)) = world.cities.get(city).map(|c| (c.owner, c.tile)) else {
        return;
    };
    let dims = world.map.dimensions;
    let radius = crate::world::city::CULTURE_RADIUS_MAX as i32;
    for tile in crate::math::tiles_in_hex_radius(&dims, center, radius) {
        if world.map.owning_city(tile) == Some(city) {
            assign_tile(world, journal, city, owner, tile);
        }
    }
}
