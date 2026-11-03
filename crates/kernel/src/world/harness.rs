//! Test harness: helpers that change an EXISTING game outside `execute` (set a
//! unit's HP, learn a technology, earn gold, grow a city, reveal tiles, meet,
//! place a city or a unit in a running test world).
//!
//! Compiled only for the kernel's own tests and for builds that ask for the
//! `harness` cargo feature (the dev-dependencies of the test crates, benches and
//! the `dev-tools` builds); never in a release build of the game. Worlds
//! BEFORE their first turn are made by `WorldBuilder`, not by these.
//!
//! Every helper goes through ONE helper (`GameState::construct`, or
//! `write_outside_commands` when no derived state reads what it writes): the
//! effect memo is always closed, and what derived state or knowledge reads is
//! committed as a synthetic journal. The oracle tests (`classic-rules/src/kernel_tests/harness_oracle.rs`) check, per helper, that after the
//! call `derived == rebuild from scratch` and that the world equals the one
//! loaded from its own save bytes.

use crate::world::construction as write;
use crate::ids::{BuildingKindId, CityHandle, PlayerId, ResourceId, TerrainId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{CityChange, PlayerChange};
use crate::world::state::GameState;

impl<M: RulesModule> GameState<M> {
    /// A change no derived state or knowledge reads: no journal.
    fn silently(&mut self, f: impl FnOnce(&mut crate::world::state::AuthWorld<M>)) {
        let ((), journal) = self.write_outside_commands(|auth, _| f(auth));
        debug_assert!(journal.is_empty());
    }

    /// The nation of a player (identity, not a game action).
    /// The nation's unit-effect sources change: follow with `debug_sources_changed`.
    pub fn debug_set_player_nation(&mut self, p: PlayerId, nation: u8) {
        self.silently(|auth| auth.players.set_nation(p, nation));
    }

    /// The sources of `p`'s unit effects changed outside a command (a nation
    /// set in a running game): derived state that reads them is computed again.
    pub fn debug_sources_changed(&mut self, rules: &CompiledRuleset<M>, p: PlayerId) {
        self.construct(rules, |_, j| j.player_changes.push(PlayerChange::EffectsChanged { player: p }));
    }

    /// A technology known from the start. Not research: no science spent, no trigger fired.
    pub(crate) fn debug_grant_tech(&mut self, rules: &CompiledRuleset<M>, p: PlayerId, tech: crate::ids::TechId) {
        self.construct(rules, |auth, j| write::grant_tech(auth, j, p, tech));
    }

    /// Set a unit's HP (healing tests need wounded units without a battle).
    /// No derived system reads HP: no journal.
    pub fn debug_set_unit_hp(&mut self, unit: UnitHandle, hp: u16) {
        self.silently(|auth| {
            if let Some(core) = auth.units.get_mut_core(unit) {
                core.hp = hp.clamp(1, crate::world::unit::UNIT_MAX_HP);
            }
        });
    }

    /// `a` and `b` have met (contact memory both ways), as if their units had seen each other.
    pub fn debug_meet(&mut self, a: PlayerId, b: PlayerId) {
        self.silently(|auth| {
            let round = auth.turn.round;
            if let Some(k) = auth.knowledge.get_mut(a.0 as usize) {
                k.met(b, round);
            }
            if let Some(k) = auth.knowledge.get_mut(b.0 as usize) {
                k.met(a, round);
            }
        });
    }

    /// Every pair of players has met.
    pub fn debug_meet_all(&mut self) {
        let players = self.auth.players.count();
        for a in 0..players {
            for b in (a + 1)..players {
                self.debug_meet(PlayerId(a as u8), PlayerId(b as u8));
            }
        }
    }

    /// Give a unit combat experience (promotion tests). No derived system reads it.
    pub fn debug_set_experience(&mut self, unit: UnitHandle, experience: u32) {
        self.silently(|auth| {
            if let Some(core) = auth.units.get_mut_core(unit) {
                core.experience = experience;
            }
        });
    }

    /// Set a city's population (journaled: yields follow).
    pub fn debug_set_population(&mut self, rules: &CompiledRuleset<M>, city: CityHandle, population: u32) {
        self.construct(rules, |auth, j| {
            if let Some(core) = auth.cities.get_mut(city) {
                core.population = population.max(1);
            }
            j.city_changes.push(CityChange::PopulationChanged { city });
        });
    }

    /// Damage a city (city combat tests). No derived system reads city damage.
    pub fn debug_set_city_damage(&mut self, city: CityHandle, damage: u16) {
        self.silently(|auth| {
            if let Some(core) = auth.cities.get_mut(city) {
                core.damage = damage;
            }
        });
    }

    /// Rename a city (city-states take their own name). The name is shown, not computed from.
    pub fn debug_set_city_name(&mut self, city: CityHandle, name: String) {
        self.silently(|auth| {
            if let Some(c) = auth.cities.get_mut(city) {
                c.name = name;
            }
        });
    }

    /// Give a player one of the module's kinds. A kind always at war is at war with everyone.
    pub fn debug_set_player_kind(&mut self, player: PlayerId, kind: crate::world::player::PlayerKind<M>) {
        self.silently(|auth| write::set_player_kind(auth, player, kind));
    }

    /// The player knows a technology (without research and without its unlock
    /// triggers). Journals TechnologyChosen: yields follow.
    pub fn debug_learn(&mut self, rules: &CompiledRuleset<M>, p: PlayerId, tech: crate::ids::TechId) {
        self.construct(rules, |auth, j| {
            let _ = auth.players.learn_tech(p, tech);
            j.player_changes.push(PlayerChange::TechnologyChosen { player: p });
        });
    }

    /// A technology known without research (showcases reach later eras).
    pub fn debug_learn_tech(&mut self, rules: &CompiledRuleset<M>, p: PlayerId, tech: crate::ids::TechId) {
        self.debug_grant_tech(rules, p, tech);
    }

    /// Fill a treasury (tests of buying tiles do not wait for income).
    pub fn debug_earn(&mut self, p: PlayerId, amount: u32) {
        self.silently(|auth| auth.players.earn(p, amount));
    }

    /// A city's conquest status (conquest sets it; happiness reads occupied cities and puppets).
    pub fn debug_set_city_status(&mut self, city: CityHandle, status: crate::world::city::CityStatus) {
        self.silently(|auth| {
            if let Some(core) = auth.cities.get_mut(city) {
                core.status = status;
            }
        });
    }

    /// A building in a city (exercises the modifier path of yields).
    pub fn debug_add_building(&mut self, rules: &CompiledRuleset<M>, city: CityHandle, building: BuildingKindId) {
        self.construct(rules, |auth, j| write::add_building(auth, j, city, building));
    }

    /// A unit of `kind` for `owner` on `tile`, with the move points and free promotions a trained unit gets.
    pub fn debug_spawn_unit(&mut self, rules: &CompiledRuleset<M>, owner: PlayerId, tile: TileId, kind: UnitKindId) -> UnitHandle {
        self.construct_with_journal(rules, |auth| write::spawn_unit(auth, rules, owner, tile, kind))
    }

    /// The unit removed (as a death leaves the world: store, indexes, the
    /// module's state); a training ground clears its scene with it.
    pub fn debug_remove_unit(&mut self, rules: &CompiledRuleset<M>, unit: UnitHandle) {
        self.construct_with_journal(rules, |auth| write::remove_unit(auth, unit));
    }

    /// A city of `owner` on `tile` with its founding ring and, for a first city, the Palace.
    pub fn debug_found_city(&mut self, rules: &CompiledRuleset<M>, owner: PlayerId, tile: TileId) -> CityHandle {
        self.construct(rules, |auth, j| write::found_city(auth, j, rules, owner, tile))
    }

    pub fn debug_set_resource_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, ResourceId)]) {
        self.construct(rules, |auth, j| write::place_resources(auth, j, rules, tiles));
    }

    pub fn debug_set_terrain_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, TerrainId)]) {
        self.construct(rules, |auth, j| write::set_terrain(auth, j, tiles));
    }

    /// A resource deposit of `amount` copies on a tile.
    pub fn debug_set_deposit(&mut self, rules: &CompiledRuleset<M>, tile: TileId, resource: ResourceId, amount: u8) {
        self.construct(rules, |auth, j| write::set_deposit(auth, j, tile, resource, amount));
    }

    /// Roads on tiles (path tests; in play workers build them).
    pub fn debug_set_road_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[TileId]) {
        self.construct(rules, |auth, j| write::set_roads(auth, j, tiles));
    }

    pub fn debug_set_feature_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, crate::ids::FeatureId)]) {
        self.construct(rules, |auth, j| write::set_features(auth, j, tiles));
    }

    /// Rivers on edges (the tile cache is built inside).
    pub fn debug_set_river_edges(&mut self, rules: &CompiledRuleset<M>, edges: &[(TileId, TileId)]) {
        self.construct(rules, |auth, j| write::set_river_edges(auth, j, edges));
    }

    /// River tiles WITHOUT beds (gameplay tests of fords and farms; the renderer draws no river).
    pub fn debug_set_river_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[TileId]) {
        self.construct(rules, |auth, j| write::set_river_flags(auth, j, tiles));
    }

    /// Built improvements in one batch. Placement rules are not checked.
    pub fn debug_set_improvement_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, crate::ids::ImprovementId)]) {
        self.construct(rules, |auth, j| write::set_improvements(auth, j, tiles));
    }

    /// Roads and railroads in one batch.
    pub fn debug_set_route_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, crate::world::map::Route)]) {
        self.construct(rules, |auth, j| write::set_routes(auth, j, tiles));
    }

    pub fn debug_set_natural_wonder_batch(&mut self, rules: &CompiledRuleset<M>, tiles: &[(TileId, crate::rules::content::NaturalWonderId)]) {
        self.construct(rules, |auth, j| write::set_natural_wonders(auth, j, tiles));
    }

    /// Remove features (forest) in one batch.
    pub fn debug_clear_features(&mut self, rules: &CompiledRuleset<M>, tiles: &[TileId]) {
        self.construct(rules, |auth, j| write::clear_features(auth, j, tiles));
    }

    /// The player has explored these tiles (map reveal of a scenario) and remembers them as
    /// they are now. A city on one is remembered by its name and owner alone, unless the
    /// player already holds a sighting of it.
    pub fn debug_reveal(&mut self, p: PlayerId, tiles: &[TileId]) {
        self.silently(|auth| {
            let round = auth.turn().round;
            let sights: Vec<_> = tiles
                .iter()
                .map(|t| {
                    let city = crate::knowledge::view::current_name_and_owner(auth, *t);
                    (crate::knowledge::view::current_layers(auth, *t), city)
                })
                .collect();
            if let Some(know) = auth.knowledge.get_mut(p.0 as usize) {
                for (tile, (layers, city)) in tiles.iter().zip(sights) {
                    // A sighting the player already holds keeps its numbers.
                    let city = know.sighting(*tile).cloned().filter(|_| layers.city).or(city);
                    know.remember(*tile, layers, city, round);
                }
            }
        });
    }

    /// Grow a city by `amount` citizens without food.
    pub fn debug_grow_city(&mut self, rules: &CompiledRuleset<M>, city: CityHandle, amount: u32) {
        self.construct(rules, |auth, j| {
            for _ in 0..amount {
                if let Some(core) = auth.cities.get_mut(city) {
                    core.population = core.population.saturating_add(1);
                    j.city_changes.push(CityChange::PopulationChanged { city });
                }
            }
        });
    }
}
