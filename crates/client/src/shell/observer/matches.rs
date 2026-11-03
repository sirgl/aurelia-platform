//! A match of AI powers over any rules module: the world, the AI that plays every
//! seat, and one projection per power (its knowledge, its fog). The module describes
//! its powers' names and figures (`Describe`); the rest is the platform's.
use std::sync::Arc;
use std::time::Duration;

use kernel::derived::visibility::{covered_tiles, CITY_SIGHT_RADIUS};
use kernel::ids::{EntityId, PlayerId, TileId};
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::world::state::GameState;
use render::world::projection;
use render::world::state::RenderWorld;

use super::card::Card;
use super::chronicle::Chronicle;
use super::contract::{Books, CityEntry, Finale, Happened, Observed, Power, Progress, Round, Sees, Sight, Stat, UnitFacts};
use crate::simulation_driver::{DriverConfig, SimulationDriver, TickOutcome};

/// How a module names its powers and shows their figures.
pub trait Describe<M: RulesModule> {
    /// The name of a power.
    fn name(&self, game: &GameState<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> String;
    /// The figures of each power's header (at least four, most important first), one
    /// list per player in `players`, in the module's language.
    fn standings(&self, game: &GameState<M>, rules: &CompiledRuleset<M>, players: &[PlayerId]) -> Vec<Vec<Stat>>;
    /// The round now and the cap the rules end the match at.
    fn round(&self, game: &GameState<M>, rules: &CompiledRuleset<M>) -> Round;
    /// What decided the match, in the module's words (`kind` is the rules' name for it).
    fn cause(&self, _game: &GameState<M>, _rules: &CompiledRuleset<M>, kind: &str) -> String {
        kind.to_owned()
    }
    /// The figures of the powers' units and world objects.
    fn books(&self, rules: &CompiledRuleset<M>) -> Books;
    /// What the power is working on, for the view of one power: its research and
    /// progress (`study`), its income. Worked out once a round, for that power only.
    fn details(&self, _game: &GameState<M>, _rules: &CompiledRuleset<M>, _player: PlayerId) -> Vec<Stat> {
        Vec::new()
    }
    /// The line the menu shows for the running match.
    fn summary(&self, game: &GameState<M>, rules: &CompiledRuleset<M>, powers: &[Power]) -> String;
    /// The tooltip of a tile in `viewer`'s view `world`: what the module's own tooltip says
    /// of a tile (terrain, yields, improvement, resource, owner), read from the view and
    /// never beyond it: a tile the viewer has not explored has none, one it only remembers
    /// shows what it remembers. With the whole map open the view is the whole map.
    fn tile_card(&self, _game: &GameState<M>, _rules: &CompiledRuleset<M>, _world: &RenderWorld, _viewer: PlayerId, _tile: TileId) -> Option<Card> {
        None
    }
    /// The card of the city on `tile`, as the module's own city screen shows it, with no
    /// orders. `full`: the viewer's own city (or any, with the whole map open); otherwise
    /// only what the viewer knows of a foreign city (`world` is the viewer's view: its city
    /// marker carries the name, owner and, when the city is in sight, the size).
    fn city_card(&self, _game: &GameState<M>, _rules: &CompiledRuleset<M>, _world: &RenderWorld, _viewer: PlayerId, _tile: TileId, _full: bool) -> Option<Card> {
        None
    }
}

/// An AI-only match of rules module `M`.
pub struct ObserverMatch<M: RulesModule, D: Describe<M>> {
    game: GameState<M>,
    rules: Arc<CompiledRuleset<M>>,
    driver: SimulationDriver<M>,
    describe: D,
    powers: Vec<Power>,
    worlds: Vec<RenderWorld>,
    /// The round number each power's view was last refreshed at.
    shown: Vec<u64>,
    standings: Vec<Vec<Stat>>,
    /// Each power's details and the round number they were worked out at.
    details: Vec<Option<(u64, Vec<Stat>)>>,
    chronicle: Chronicle,
    /// The power the view opens on alone.
    first: Option<usize>,
    serial: u64,
    open: bool,
    /// The whole map is open in every view.
    revealed: bool,
}

impl<M: RulesModule, D: Describe<M>> ObserverMatch<M, D> {
    /// A match of `game`: every major power is the AI's, at the rules' default difficulty
    /// (or `difficulty`, a handicap key).
    pub fn new(mut game: GameState<M>, rules: Arc<CompiledRuleset<M>>, describe: D, difficulty: Option<&str>) -> Self {
        let difficulty = difficulty.unwrap_or_else(|| rules.default_difficulty()).to_owned();
        let driver = SimulationDriver::with_difficulty(DriverConfig { human_players: 0 }, &rules, &difficulty);
        driver.configure_game(&mut game);
        let players = majors(&game);
        let powers: Vec<Power> = players
            .iter()
            .map(|&player| Power { player: player.0, name: describe.name(&game, &rules, player) })
            .collect();
        let worlds = players.iter().map(|&player| view_of(&game, &rules, player)).collect();
        let standings = describe.standings(&game, &rules, &players);
        let chronicle = Chronicle::new(&game, &powers);
        let details = vec![None; powers.len()];
        Self { game, rules, driver, describe, shown: vec![0; powers.len()], powers, worlds, standings, details, chronicle, first: None, serial: 0, open: false, revealed: false }
    }

    /// The view opens on `power` alone, not on the grid.
    pub fn watching(mut self, power: usize) -> Self {
        self.first = (power < self.powers.len()).then_some(power);
        self
    }

    pub fn game(&self) -> &GameState<M> {
        &self.game
    }

    pub fn rules(&self) -> &CompiledRuleset<M> {
        &self.rules
    }

    fn players(&self) -> Vec<PlayerId> {
        self.powers.iter().map(|p| PlayerId(p.player)).collect()
    }

    fn round_ended(&mut self) {
        self.open = false;
        self.serial += 1;
        self.standings = self.describe.standings(&self.game, &self.rules, &self.players());
        let round = self.describe.round(&self.game, &self.rules).now;
        self.chronicle.note(&self.game, &self.rules, &self.powers, round.saturating_sub(1));
    }
}

/// The major powers of the game, in player order.
fn majors<M: RulesModule>(game: &GameState<M>) -> Vec<PlayerId> {
    let players = game.view().auth.players();
    (0..players.count()).map(|i| PlayerId(i as u8)).filter(|&p| players.is_major(p)).collect()
}

/// The projection of the world through one power's knowledge, for the visual surface.
fn view_of<M: RulesModule>(game: &GameState<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> RenderWorld {
    let mut world = projection::rebuild_render_world(game.view(), player, rules);
    world.visual_surface = true;
    world
}

impl<M: RulesModule, D: Describe<M>> Observed for ObserverMatch<M, D> {
    fn powers(&self) -> &[Power] {
        &self.powers
    }

    fn alive(&self, power: usize) -> bool {
        self.powers.get(power).is_some_and(|p| self.game.view().auth.players().is_alive(PlayerId(p.player)))
    }

    fn standing(&self, power: usize) -> &[Stat] {
        self.standings.get(power).map_or(&[], Vec::as_slice)
    }

    fn round(&self) -> Round {
        self.describe.round(&self.game, &self.rules)
    }

    fn advance(&mut self, slice: Duration) -> Progress {
        if self.game.is_over() {
            return Progress::Over;
        }
        self.open = true;
        match self.driver.tick_game(&mut self.game, &self.rules, slice) {
            TickOutcome::Busy => Progress::Working,
            TickOutcome::RoundDone => {
                self.round_ended();
                Progress::RoundDone
            }
            // Nobody is left to play: the match cannot go on.
            TickOutcome::Idle => {
                self.round_ended();
                Progress::Over
            }
        }
    }

    fn mid_round(&self) -> bool {
        self.open
    }

    fn is_over(&self) -> bool {
        self.game.is_over()
    }

    fn finale(&self) -> Option<Finale> {
        if !self.game.is_over() {
            return None;
        }
        let name = |p: PlayerId| self.describe.name(&self.game, &self.rules, p);
        let outcome = M::outcome(&self.game.view().auth);
        let (winners, cause) = match outcome {
            Some(outcome) => (outcome.winners.into_iter().map(name).collect(), self.describe.cause(&self.game, &self.rules, outcome.kind)),
            None => (Vec::new(), String::new()),
        };
        Some(Finale { winners, cause, round: self.describe.round(&self.game, &self.rules).now })
    }

    fn serial(&self) -> u64 {
        self.serial
    }

    fn refresh(&mut self, power: usize) -> bool {
        let Some(player) = self.powers.get(power).map(|p| PlayerId(p.player)) else {
            return false;
        };
        if self.shown[power] == self.serial {
            return false;
        }
        self.shown[power] = self.serial;
        let changed = projection::refresh_with(&mut self.worlds[power], self.game.view(), player, &self.rules, self.revealed);
        !changed.is_empty()
    }

    fn world(&self, power: usize) -> &RenderWorld {
        &self.worlds[power]
    }

    fn books(&self) -> Books {
        self.describe.books(&self.rules)
    }

    fn state_hash(&self) -> u64 {
        kernel::replay::hash::authoritative_hash(&self.game)
    }

    fn summary(&self) -> String {
        self.describe.summary(&self.game, &self.rules, &self.powers)
    }

    fn first_view(&self) -> Option<usize> {
        self.first
    }

    fn details(&self, power: usize) -> &[Stat] {
        self.details.get(power).and_then(Option::as_ref).map_or(&[], |(_, stats)| stats.as_slice())
    }

    fn refresh_details(&mut self, power: usize) {
        let Some(player) = self.powers.get(power).map(|p| PlayerId(p.player)) else { return };
        if self.details[power].as_ref().is_some_and(|(at, _)| *at == self.serial) {
            return;
        }
        self.details[power] = Some((self.serial, self.describe.details(&self.game, &self.rules, player)));
    }

    fn events(&self, power: usize) -> &[Happened] {
        self.chronicle.of(power)
    }

    fn revealed(&self) -> bool {
        self.revealed
    }

    fn set_revealed(&mut self, on: bool) {
        if self.revealed != on {
            self.revealed = on;
            // Every view owes a rebuild, whichever round it was last refreshed at.
            self.shown.iter_mut().for_each(|at| *at = u64::MAX);
        }
    }

    fn current(&self, power: usize) -> bool {
        !self.mid_round() && self.shown.get(power) == Some(&self.serial)
    }

    fn tile_card(&self, power: usize, tile: u32) -> Option<Card> {
        let player = self.player_of(power)?;
        let world = self.worlds.get(power)?;
        if !world.is_known(tile as usize) {
            return None;
        }
        self.describe.tile_card(&self.game, &self.rules, world, player, TileId(tile))
    }

    fn units_on(&self, power: usize, tile: u32) -> Vec<UnitFacts> {
        let Some(world) = self.worlds.get(power) else { return Vec::new() };
        world.units.iter().filter(|u| u.tile.0 == tile).filter_map(|u| self.facts_of(power, u.id)).collect()
    }

    fn unit_facts(&self, power: usize, id: u64) -> Option<UnitFacts> {
        let world = self.worlds.get(power)?;
        world.units.iter().any(|u| u.id == id).then(|| self.facts_of(power, id)).flatten()
    }

    fn cities_of(&self, power: usize) -> Vec<CityEntry> {
        let Some(player) = self.player_of(power) else { return Vec::new() };
        let mut cities: Vec<CityEntry> = self.game.view().auth.cities().iter().filter(|(_, c)| c.owner == player).map(|(_, c)| self.entry_of(power, c.tile.0, c.owner, &c.name, Some(c.population))).collect();
        cities.sort_by_key(|c| (std::cmp::Reverse(c.population), c.tile));
        cities
    }

    fn city_at(&self, power: usize, tile: u32) -> Option<CityEntry> {
        let world = self.worlds.get(power)?;
        let marker = world.cities.iter().find(|c| c.tile.0 == tile)?;
        Some(self.entry_of(power, tile, PlayerId(marker.owner), &marker.name, marker.population))
    }

    fn city_card(&self, power: usize, tile: u32) -> Option<Card> {
        let player = self.player_of(power)?;
        let entry = self.city_at(power, tile)?;
        let full = entry.own || self.revealed;
        self.describe.city_card(&self.game, &self.rules, self.worlds.get(power)?, player, TileId(tile), full)
    }

    fn sight(&self, power: usize, target: Sees) -> Option<Sight> {
        let player = self.player_of(power)?;
        let auth = self.game.view().auth;
        let (at, radius, owner) = match target {
            Sees::Unit(id) => {
                let handle = auth.units().handle_of(EntityId(id))?;
                let core = *auth.units().get(handle)?.core;
                (core.tile, self.game.view().derived.visibility.sight_of(handle), core.owner)
            }
            Sees::City(tile) => {
                let city = auth.cities().iter().find(|(_, c)| c.tile.0 == tile)?.1;
                (city.tile, CITY_SIGHT_RADIUS, city.owner)
            }
        };
        if radius <= 0 || (owner != player && !self.revealed) {
            return None;
        }
        let tiles = covered_tiles(auth.map(), at, radius).map(|t| t.0).collect();
        Some(Sight { at: at.0, radius: radius as u8, tiles })
    }
}

impl<M: RulesModule, D: Describe<M>> ObserverMatch<M, D> {
    fn player_of(&self, power: usize) -> Option<PlayerId> {
        self.powers.get(power).map(|p| PlayerId(p.player))
    }

    /// The name of the power a player serves (a player that is no power of the match
    /// is named by the module all the same).
    fn power_name(&self, player: PlayerId) -> String {
        match self.powers.iter().find(|p| p.player == player.0) {
            Some(power) => power.name.clone(),
            None => self.describe.name(&self.game, &self.rules, player),
        }
    }

    fn entry_of(&self, power: usize, tile: u32, owner: PlayerId, name: &str, population: Option<u32>) -> CityEntry {
        CityEntry { tile, name: name.to_owned(), owner: self.power_name(owner), player: owner.0, own: self.player_of(power) == Some(owner), population }
    }

    /// A unit of the power's view by identity.
    fn facts_of(&self, power: usize, id: u64) -> Option<UnitFacts> {
        let viewer = self.player_of(power)?;
        let auth = self.game.view().auth;
        let handle = auth.units().handle_of(EntityId(id))?;
        let core = *auth.units().get(handle)?.core;
        let own = core.owner == viewer;
        let kind = self.rules.units.get(usize::from(core.kind.0)).map(|u| u.name.clone()).unwrap_or_default();
        let sight = (own || self.revealed).then(|| self.game.view().derived.visibility.sight_of(handle)).filter(|r| *r > 0).map(|r| r as u8);
        Some(UnitFacts { id, tile: core.tile.0, kind, owner: self.power_name(core.owner), player: core.owner.0, own, hp: core.hp, max_hp: kernel::world::unit::UNIT_MAX_HP, fortified: core.stance.is_fortified(), sight })
    }
}

/// The research a power is on, as a percentage of its cost: the name, the share done.
pub fn study<M: RulesModule>(game: &GameState<M>, rules: &CompiledRuleset<M>, player: PlayerId) -> Option<Study> {
    use kernel::systems::module::ModuleSystems;
    let auth = game.view().auth;
    let players = auth.players();
    let tech = *players.research_queue_of(player).first()?;
    let def = rules.technologies.get(usize::from(tech.0))?;
    let cost = <M::Systems as ModuleSystems<M>>::research_cost(auth, rules, player, tech).max(1);
    let name = if def.name.is_empty() { def.key.clone() } else { def.name.clone() };
    Some(Study { name, percent: (u64::from(players.science_of(player)) * 100 / u64::from(cost)).min(99) as u8 })
}

/// The research a power is on and how far it has come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Study {
    pub name: String,
    pub percent: u8,
}
