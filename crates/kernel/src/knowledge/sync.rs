//! Knowledge sync: the one writer of player knowledge, run after every
//! journal (GameState::commit_journal), with the derived visibility already
//! updated for that journal.
//!
//! - An observer that arrives (unit created or moved, city founded or
//!   gained, tile gained) remembers the tiles it now covers.
//! - An observer that leaves (unit moved or destroyed, city lost) stamps the
//!   tiles that are no longer visible with the current round: their memory
//!   is the last state seen (content seen while visible is always fresh).
//!   The city on such a tile is sighted at that moment: its population, era
//!   and buildings as they were when it left sight.
//! - A tile whose content changes is remembered again by every player who
//!   sees it. A changed terrain or feature (a chopped forest) moves the
//!   lines of sight around it: every player remembers what it now sees there.
//! - Contacts: seeing a unit or city of another player records the sight
//!   (and the mutual meeting); at the end of a player's turn every foreign
//!   unit and city it sees counts as seen in that round.
//!
//! Cost: O(sight area) per observer event, O(players) per changed tile,
//! O(units + cities) per EndTurn. A city sighting is built only for a tile
//! that holds a city.

use crate::module::RulesModule;
use crate::derived::visibility::{covered_tiles, SightChange, MAX_SIGHT, PLOT_SIGHT_RADIUS};
use crate::derived::DerivedWorld;
use crate::ids::{PlayerId, TileId};
use crate::knowledge::state::{CitySighting, RememberedLayers};
use crate::knowledge::view::{city_owner_on, current_layers, current_sighting, is_visible};
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, PlayerChange, TileChange};
use crate::world::state::AuthWorld;

/// The farthest a line of sight reaches.
const LONGEST_SIGHT: i32 = MAX_SIGHT;

pub(crate) fn sync<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    journal: &ChangeJournal<M>,
) {
    // The coverage changes the visibility delta made for this journal, with
    // the radius each observer had (units see by their own sight).
    for event in &derived.visibility.events {
        match event.change {
            SightChange::Arrived => {
                gain(auth, derived, rules, event.player, event.at, event.radius);
                noticed(auth, derived, event.player, event.at);
            }
            SightChange::Widened => gain(auth, derived, rules, event.player, event.at, event.radius),
            SightChange::Left => lose(auth, derived, rules, event.player, event.at, event.radius),
        }
    }
    for change in &journal.tile_changes {
        match *change {
            TileChange::OwnerChanged { tile, from, to } => {
                if let Some(from) = from {
                    lose(auth, derived, rules, from, tile, PLOT_SIGHT_RADIUS);
                }
                if let Some(to) = to {
                    gain(auth, derived, rules, to, tile, PLOT_SIGHT_RADIUS);
                }
            }
            TileChange::TerrainSet { tile } | TileChange::FeatureSet { tile } | TileChange::FeatureChopped { tile } => {
                look_again(auth, derived, rules, tile);
            }
            _ => {}
        }
        refresh_watchers(auth, derived, rules, tile_of(change));
    }
    for change in &journal.player_changes {
        match *change {
            PlayerChange::TurnEnded { player } => look_around(auth, derived, player),
            PlayerChange::MapRevealed { player } => reveal_map(auth, rules, player),
            PlayerChange::AreaRevealed { player, tile, radius } => reveal_area(auth, rules, player, tile, radius),
            PlayerChange::TileNoticed { player, tile } => notice_tile(auth, rules, player, tile),
            _ => {}
        }
    }
}

/// What a tile shows now: its layers and the city standing on it.
fn sight_of<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    tile: TileId,
) -> (RememberedLayers, Option<CitySighting>) {
    (current_layers(auth, tile), current_sighting(auth, rules, tile))
}

/// The player remembers every tile as it is now. Tiles it sees stay visible; the others become
/// remembered from this round.
fn reveal_map<M: RulesModule>(auth: &mut AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId) {
    let round = auth.turn().round;
    let tiles = auth.map().tile_count();
    let sights: Vec<_> = (0..tiles).map(|t| sight_of(auth, rules, TileId(t as u32))).collect();
    if let Some(knowledge) = auth.knowledge.get_mut(player.0 as usize) {
        for (t, (layers, city)) in sights.into_iter().enumerate() {
            knowledge.remember(TileId(t as u32), layers, city, round);
        }
    }
}

/// The player remembers every tile within `radius` of `tile` as it is now;
/// tiles it sees stay visible.
fn reveal_area<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    tile: TileId,
    radius: u8,
) {
    let round = auth.turn().round;
    let around: Vec<TileId> = crate::math::hex_disc(auth.map().dimensions, tile, i32::from(radius)).collect();
    let sights: Vec<_> = around.iter().map(|&t| sight_of(auth, rules, t)).collect();
    if let Some(knowledge) = auth.knowledge.get_mut(player.0 as usize) {
        for (t, (layers, city)) in around.into_iter().zip(sights) {
            knowledge.remember(t, layers, city, round);
        }
    }
}

/// The player remembers an explored tile as it is now.
fn notice_tile<M: RulesModule>(auth: &mut AuthWorld<M>, rules: &CompiledRuleset<M>, player: PlayerId, tile: TileId) {
    let round = auth.turn().round;
    let (layers, city) = sight_of(auth, rules, tile);
    if let Some(knowledge) = auth.knowledge.get_mut(player.0 as usize) {
        if knowledge.is_explored(tile) {
            knowledge.remember(tile, layers, city, round);
        }
    }
}

fn tile_of(change: &TileChange) -> TileId {
    match *change {
        TileChange::OwnerChanged { tile, .. }
        | TileChange::ImprovementBuilt { tile }
        | TileChange::ResourceSet { tile }
        | TileChange::RoadBuilt { tile }
        | TileChange::RiverSet { tile }
        | TileChange::FeatureSet { tile }
        | TileChange::FeatureChopped { tile }
        | TileChange::TerrainSet { tile }
        | TileChange::NaturalWonderSet { tile } => tile,
    }
}

/// Lines of sight around `tile` changed: every player remembers the tiles
/// around it that it sees now.
fn look_again<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    tile: TileId,
) {
    let round = auth.turn().round;
    let around: Vec<TileId> = crate::math::hex_disc(auth.map().dimensions, tile, LONGEST_SIGHT).collect();
    for p in 0..auth.knowledge.len() {
        let player = PlayerId(p as u8);
        for &t in &around {
            if is_visible(derived, player, t) {
                let (layers, city) = sight_of(auth, rules, t);
                auth.knowledge[p].remember(t, layers, city, round);
                see_owners_on(auth, derived, player, t, round);
            }
        }
    }
}

/// `player` now covers the disc around `at`: remember what it sees there,
/// and note the other players' units and cities in it.
fn gain<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    at: TileId,
    radius: i32,
) {
    let round = auth.turn().round;
    let tiles: Vec<TileId> = covered_tiles(auth.map(), at, radius).collect();
    for tile in tiles {
        if !is_visible(derived, player, tile) {
            continue;
        }
        let (layers, city) = sight_of(auth, rules, tile);
        if let Some(knowledge) = auth.knowledge.get_mut(player.0 as usize) {
            knowledge.remember(tile, layers, city, round);
        }
        see_owners_on(auth, derived, player, tile, round);
    }
}

/// `player` stopped covering the disc around `at`: tiles it no longer sees
/// were last seen now, and a city on one is as it stands now.
fn lose<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    at: TileId,
    radius: i32,
) {
    let round = auth.turn().round;
    let tiles: Vec<TileId> = covered_tiles(auth.map(), at, radius)
        .filter(|&tile| !is_visible(derived, player, tile))
        .collect();
    let remembered = |tile: &TileId| auth.knowledge.get(player.0 as usize).is_some_and(|k| k.sighting(*tile).is_some());
    let sightings: Vec<_> = tiles
        .iter()
        .filter(|tile| remembered(tile))
        .map(|&tile| (tile, current_sighting(auth, rules, tile)))
        .collect();
    let Some(knowledge) = auth.knowledge.get_mut(player.0 as usize) else {
        return;
    };
    for tile in tiles {
        knowledge.stamp(tile, round);
    }
    for (tile, city) in sightings {
        knowledge.refresh_sighting(tile, city);
    }
}

/// The content of `tile` changed: everybody who sees it remembers it anew.
fn refresh_watchers<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    rules: &CompiledRuleset<M>,
    tile: TileId,
) {
    let round = auth.turn().round;
    let (layers, city) = sight_of(auth, rules, tile);
    for p in 0..auth.knowledge.len() {
        if is_visible(derived, PlayerId(p as u8), tile) {
            auth.knowledge[p].remember(tile, layers, city.clone(), round);
        }
    }
}

/// A unit or city of `owner` appeared on `tile`: every other player who
/// sees the tile sees `owner`.
fn noticed<M: RulesModule>(auth: &mut AuthWorld<M>, derived: &DerivedWorld<M>, owner: PlayerId, tile: TileId) {
    let round = auth.turn().round;
    for p in 0..auth.knowledge.len() {
        let viewer = PlayerId(p as u8);
        if viewer != owner && is_visible(derived, viewer, tile) {
            meet(auth, viewer, owner, round);
        }
    }
}

/// Units and cities of other players standing on a tile `viewer` sees.
/// Reads the dense occupancy layer (one array read per player), not the
/// unit index: this runs for every tile an observer uncovers.
fn see_owners_on<M: RulesModule>(
    auth: &mut AuthWorld<M>,
    derived: &DerivedWorld<M>,
    viewer: PlayerId,
    tile: TileId,
    round: u32,
) {
    let city_owner = city_owner_on(auth, tile);
    for p in 0..auth.knowledge.len() {
        let owner = PlayerId(p as u8);
        let present = derived.movement.units_of(owner, tile) > 0 || city_owner == Some(owner);
        if owner != viewer && present {
            meet(auth, viewer, owner, round);
        }
    }
}

/// End of `player`'s turn: every foreign unit and city it sees is seen in
/// this round (contacts stay current while both stand still).
fn look_around<M: RulesModule>(auth: &mut AuthWorld<M>, derived: &DerivedWorld<M>, player: PlayerId) {
    let round = auth.turn().round;
    for p in 0..auth.knowledge.len() {
        let other = PlayerId(p as u8);
        let already = auth.knowledge[player.0 as usize]
            .contact(other)
            .last_seen_round
            == Some(round);
        if other == player || already || !in_sight(auth, derived, player, other) {
            continue;
        }
        meet(auth, player, other, round);
    }
}

/// Does `viewer` see a unit or city of `other` now? Stops at the first one.
fn in_sight<M: RulesModule>(auth: &AuthWorld<M>, derived: &DerivedWorld<M>, viewer: PlayerId, other: PlayerId) -> bool {
    let units = auth
        .indexes()
        .units_of(other)
        .iter()
        .filter_map(|u| auth.units().get(*u));
    let unit_tiles = units.map(|u| u.core.tile);
    let city_tiles = auth
        .indexes()
        .cities_of(other)
        .iter()
        .filter_map(|c| auth.cities().get(*c))
        .map(|c| c.tile);
    let mut tiles = unit_tiles.chain(city_tiles);
    tiles.any(|tile| is_visible(derived, viewer, tile))
}

fn meet<M: RulesModule>(auth: &mut AuthWorld<M>, viewer: PlayerId, owner: PlayerId, round: u32) {
    if let Some(knowledge) = auth.knowledge.get_mut(viewer.0 as usize) {
        knowledge.saw(owner, round);
    }
    if let Some(knowledge) = auth.knowledge.get_mut(owner.0 as usize) {
        knowledge.met(viewer, round);
    }
}
