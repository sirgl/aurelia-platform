//! `WorldBuilder`: how a world is made before the game starts.
//!
//! The one external write boundary of a running game is `GameState::execute`.
//! A world that does not exist yet has no commands to play: its map is laid,
//! its players are named, its first cities and units are placed. That is
//! construction, and it goes through this builder, not through mutators on
//! the game (platform/docs/world-builder.md).
//!
//! - The LAND methods (terrain, rivers, features, deposits, improvements,
//!   routes, natural wonders) write the map directly and record what they
//!   changed in one pending journal. Nothing reads the world meanwhile, so
//!   derived state and knowledge follow ONCE, at the next checkpoint: the first
//!   call that reads the world (`state`, an entity method) or `build`.
//! - The ENTITY methods (`found_city`, `spawn_unit`, `grant_tech`) first
//!   reach the checkpoint, then change the world with the journal of that
//!   one change, because sight and knowledge are order-dependent: what a player
//!   remembers is what it saw when it saw it.
//! - The module's own world state is reached through `auth_mut`, for the
//!   module's construction code only.
//!
//! `build` hands over the finished `GameState`; from then on the world changes
//! only through `execute`.

use crate::ids::{BuildingKindId, CityHandle, FeatureId, ImprovementId, PlayerId, ResourceId, TechId, TerrainId, TileId, UnitHandle, UnitKindId};
use crate::module::RulesModule;
use crate::rules::content::NaturalWonderId;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::ChangeJournal;
use crate::world::construction as write;
use crate::world::map::Route;
use crate::world::player::PlayerKind;
use crate::world::state::{AuthWorld, GameConfig, GameState};

/// A world under construction.
pub struct WorldBuilder<'r, M: RulesModule> {
    game: GameState<M>,
    rules: &'r CompiledRuleset<M>,
    /// The land layers written since the last checkpoint.
    pending: ChangeJournal<M>,
}

impl<'r, M: RulesModule> WorldBuilder<'r, M> {
    /// An empty world of `config` (all water, no cities, no units).
    pub fn new(rules: &'r CompiledRuleset<M>, config: GameConfig) -> Self {
        Self { game: GameState::new(rules, config), rules, pending: ChangeJournal::default() }
    }

    pub fn rules(&self) -> &'r CompiledRuleset<M> {
        self.rules
    }

    /// The checkpoint: derived state and knowledge follow every land layer
    /// written since the last one, in one journal.
    pub fn checkpoint(&mut self) {
        if !self.pending.is_empty() {
            let journal = std::mem::take(&mut self.pending);
            self.game.commit_journal(self.rules, &journal);
        }
    }

    /// The world as it is now, for reads in the middle of construction
    /// (picking starts on the laid map). Reaches the checkpoint first.
    pub fn state(&mut self) -> &GameState<M> {
        self.checkpoint();
        &self.game
    }

    /// Finish: the world, ready to play.
    pub fn build(mut self) -> GameState<M> {
        self.checkpoint();
        self.game.close_memo();
        self.game
    }

    /// The world's authoritative state, for the rules module's construction
    /// code (its own `module` world, settings that only its rules read). A change
    /// that derived state reads must come with a journal: use the methods above.
    pub fn auth_mut(&mut self) -> &mut AuthWorld<M> {
        self.game.close_memo();
        self.game.auth_for_builder()
    }

    // ---- players

    /// The nation of a player (identity, not a game action).
    pub fn set_player_nation(&mut self, player: PlayerId, nation: u8) {
        self.auth_mut().players.set_nation(player, nation);
    }

    /// A kind of the module's for a player; a kind always at war is at war with everyone.
    pub fn set_player_kind(&mut self, player: PlayerId, kind: PlayerKind<M>) {
        write::set_player_kind(self.auth_mut(), player, kind);
    }

    /// Difficulty (a handicap key) and the players the AI plays.
    pub fn configure_players(&mut self, difficulty: &str, ai: &[PlayerId]) {
        write::configure_players(self.auth_mut(), difficulty, ai);
    }

    /// The game's speed (a speed key of the rules; Standard keeps it empty).
    pub fn set_speed(&mut self, speed: &str) {
        let rules = self.rules;
        write::set_speed(self.auth_mut(), rules, speed);
    }

    /// Fill a treasury.
    pub fn earn(&mut self, player: PlayerId, amount: u32) {
        self.auth_mut().players.earn(player, amount);
    }

    /// Two players at war (or at peace).
    pub fn set_war(&mut self, a: PlayerId, b: PlayerId, war: bool) {
        self.auth_mut().diplomacy.set_war(a, b, war);
    }

    // ---- land (pending until the checkpoint)

    fn land<R>(&mut self, f: impl FnOnce(&mut AuthWorld<M>, &mut ChangeJournal<M>) -> R) -> R {
        self.game.close_memo();
        f(self.game.auth_for_builder(), &mut self.pending)
    }

    pub fn set_terrain(&mut self, tiles: &[(TileId, TerrainId)]) {
        self.land(|a, j| write::set_terrain(a, j, tiles));
    }

    /// Rivers on the edges between neighbouring tiles.
    pub fn set_rivers(&mut self, edges: &[(TileId, TileId)]) {
        self.land(|a, j| write::set_river_edges(a, j, edges));
    }

    pub fn set_features(&mut self, tiles: &[(TileId, FeatureId)]) {
        self.land(|a, j| write::set_features(a, j, tiles));
    }

    pub fn clear_features(&mut self, tiles: &[TileId]) {
        self.land(|a, j| write::clear_features(a, j, tiles));
    }

    pub fn set_natural_wonders(&mut self, tiles: &[(TileId, NaturalWonderId)]) {
        self.land(|a, j| write::set_natural_wonders(a, j, tiles));
    }

    /// Deposits, each of the amount its tile picks (`scenario::deposit_amount`).
    pub fn place_resources(&mut self, tiles: &[(TileId, ResourceId)]) {
        let rules = self.rules;
        self.land(|a, j| write::place_resources(a, j, rules, tiles));
    }

    /// Deposits of a given amount each (map setup that decides the amount, as the guaranteed
    /// strategic resources of a start do).
    pub fn place_deposits(&mut self, tiles: &[(TileId, ResourceId, u8)]) {
        self.land(|a, j| write::place_deposits(a, j, tiles));
    }

    pub fn set_improvements(&mut self, tiles: &[(TileId, ImprovementId)]) {
        self.land(|a, j| write::set_improvements(a, j, tiles));
    }

    pub fn set_routes(&mut self, tiles: &[(TileId, Route)]) {
        self.land(|a, j| write::set_routes(a, j, tiles));
    }

    // ---- entities (each reaches the checkpoint first)

    /// A city of `owner` on `tile` with its founding ring and, for a first city,
    /// the Palace (no settler, no module reaction).
    pub fn found_city(&mut self, owner: PlayerId, tile: TileId) -> CityHandle {
        self.checkpoint();
        let rules = self.rules;
        self.game.construct(rules, |a, j| write::found_city(a, j, rules, owner, tile))
    }

    /// A unit with the move points and free promotions a trained unit gets.
    pub fn spawn_unit(&mut self, owner: PlayerId, tile: TileId, kind: UnitKindId) -> UnitHandle {
        self.checkpoint();
        let rules = self.rules;
        self.game.construct_with_journal(rules, |a| write::spawn_unit(a, rules, owner, tile, kind))
    }

    /// A technology known from the start. Not research.
    pub fn grant_tech(&mut self, player: PlayerId, tech: TechId) {
        self.checkpoint();
        let rules = self.rules;
        self.game.construct(rules, |a, j| write::grant_tech(a, j, player, tech));
    }

    /// A building in a city.
    pub fn add_building(&mut self, city: CityHandle, building: BuildingKindId) {
        self.checkpoint();
        let rules = self.rules;
        self.game.construct(rules, |a, j| write::add_building(a, j, city, building));
    }
}
