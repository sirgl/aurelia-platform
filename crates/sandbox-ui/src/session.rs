//! A running sandbox match: the game client (the world and the observer's
//! projection), the AI players and what the player selected. Every order
//! goes through `GameClient::submit`, the one write boundary.

use std::sync::Arc;
use std::time::Duration;

use i18n::tf;
use client::shell::module::{AiProgress, CityBanner, MapButton, Outcome, TopBarValue};
use client::shell::hud::Icon;
use client::simulation_driver::{DriverConfig, SimulationDriver, TickOutcome};
use client::GameClient;
use kernel::command::KernelCommand;
use kernel::ids::{CityHandle, PlayerId, ProductionItem, TileId, UnitHandle};
use kernel::persistence::{codec, decode::decode, encode::encode};
use kernel::world::city::CityCore;
use render::renderer::Renderer;
use sandbox_rules::combat::{defender_on, strength};
use crate::screens::signed;
use sandbox_rules::systems::FOOD_PER_CITIZEN;
use sandbox_rules::victory::victor;
use sandbox_rules::{Command, Game, Rules, Sandbox};

/// One item a city may produce, as the city screen offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub item: ProductionItem,
    pub name: String,
    pub cost: u32,
}

pub struct Session {
    pub client: GameClient<Sandbox>,
    driver: SimulationDriver<Sandbox>,
    selected: Option<UnitHandle>,
    pub city: Option<CityHandle>,
    /// Why the last order was refused.
    pub message: Option<String>,
}

impl Session {
    /// A session over `game`; `humans` players (from player 0) are played
    /// by hand, the rest by the AI.
    pub fn new(mut game: Game, rules: Arc<Rules>, humans: u8) -> Self {
        let driver = SimulationDriver::new(DriverConfig { human_players: humans }, &rules);
        driver.configure_game(&mut game);
        let mut client = GameClient::new(game, rules, Renderer::headless(), PlayerId(0));
        client.render_world.visual_surface = true;
        let mut session = Self { client, driver, selected: None, city: None, message: None };
        session.select_first_unit();
        session
    }

    pub fn from_save(bytes: &[u8], rules: Arc<Rules>) -> Result<Self, String> {
        let save = codec::from_bytes::<Sandbox>(bytes).map_err(|e| tf!("sandbox.session.not-a-save", detail = format!("{e:?}")))?;
        let game = decode(&save, &rules).map_err(|e| tf!("sandbox.session.save-failed", detail = format!("{e:?}")))?;
        let mut session = Self::new(game, rules, 1);
        session.driver.load_ai(&save.ai, &session.client).map_err(|e| format!("{e:?}"))?;
        Ok(session)
    }

    pub fn save_bytes(&self) -> Vec<u8> {
        let mut save = encode(&self.client.game, &self.client.rules);
        save.ai = self.driver.save_ai();
        codec::to_bytes(&save)
    }

    pub fn rules(&self) -> &Rules {
        &self.client.rules
    }

    fn observer(&self) -> PlayerId {
        self.client.observer
    }

    pub fn round(&self) -> u32 {
        self.client.game.view().auth.turn().round
    }

    pub fn outcome(&self) -> Outcome {
        match victor(self.client.game.view().auth) {
            None => Outcome::Playing,
            Some(winner) if winner == self.observer() => Outcome::Victory,
            Some(_) => Outcome::Defeat,
        }
    }

    pub fn faction(&self) -> String {
        let auth = self.client.game.view().auth;
        let nation = auth.players().nation_of(self.observer());
        self.rules().nations.get(nation as usize).map(|c| c.name.clone()).unwrap_or_default()
    }

    // ---- turns ----

    pub fn ai_busy(&self) -> bool {
        self.driver.busy(&self.client)
    }

    pub fn end_turn(&mut self) {
        self.selected = None;
        if let Err(error) = self.driver.end_human_turn(&mut self.client) {
            self.message = Some(error.to_string());
        }
    }

    pub fn tick_ai(&mut self, slice: Duration) -> AiProgress {
        match self.driver.tick(&mut self.client, slice) {
            TickOutcome::Busy => AiProgress::Busy,
            TickOutcome::RoundDone => {
                self.select_first_unit();
                AiProgress::Finished
            }
            TickOutcome::Idle => AiProgress::Idle,
        }
    }

    pub fn finish_ai_turns(&mut self) {
        self.driver.finish(&mut self.client);
    }

    /// From now on players 0..humans play by hand, the rest by the AI
    /// (0: the AI plays the observed player too). The AI memory starts afresh.
    pub fn set_humans(&mut self, humans: u8) {
        self.driver = SimulationDriver::new(DriverConfig { human_players: humans }, &self.client.rules);
    }

    /// One full round of AI turns (offscreen runs).
    pub fn play_round(&mut self) {
        self.driver.round(&mut self.client);
    }

    // ---- selection and orders ----

    fn own_unit_on(&self, tile: TileId) -> Option<UnitHandle> {
        let auth = self.client.game.view().auth;
        let mut units: Vec<UnitHandle> = auth.indexes().units_on(tile).to_vec();
        units.retain(|u| auth.units().get(*u).is_some_and(|v| v.core.owner == self.observer()));
        units.into_iter().min_by_key(|u| auth.units().get(*u).map(|v| v.core.id))
    }

    fn select_first_unit(&mut self) {
        let auth = self.client.game.view().auth;
        let units = auth.indexes().units_of(self.observer());
        self.selected = units.iter().copied().find(|u| auth.units().get(*u).is_some_and(|v| v.core.moves_left > 0));
    }

    pub fn selected_unit(&self) -> Option<kernel::world::unit::UnitCore> {
        let unit = self.selected?;
        self.client.game.view().auth.units().get(unit).map(|v| *v.core)
    }

    pub fn selected_tile(&self) -> Option<u32> {
        self.selected_unit().map(|u| u.tile.0)
    }

    /// Primary: select an own unit or open an own city. Secondary with a
    /// unit selected: attack an adjacent enemy, else march there.
    /// Returns true when an own city was chosen.
    pub fn click(&mut self, tile: Option<u32>, button: MapButton) -> bool {
        let Some(tile) = tile.map(TileId) else {
            self.selected = None;
            return false;
        };
        match button {
            MapButton::Primary => self.choose(tile),
            MapButton::Secondary => {
                self.order_to(tile);
                false
            }
        }
    }

    fn choose(&mut self, tile: TileId) -> bool {
        let auth = self.client.game.view().auth;
        if let Some(unit) = self.own_unit_on(tile) {
            self.selected = Some(unit);
            return false;
        }
        let city = auth.indexes().city_on_tile(tile).filter(|c| auth.cities().get(*c).is_some_and(|c| c.owner == self.observer()));
        self.city = city.or(self.city);
        self.selected = None;
        city.is_some()
    }

    fn order_to(&mut self, tile: TileId) {
        let Some(unit) = self.selected_unit() else { return };
        let Some(handle) = self.selected else { return };
        let auth = self.client.game.view().auth;
        let adjacent = auth.map().dimensions.neighbors(unit.tile).into_iter().any(|t| t == tile);
        let enemy = defender_on(auth, self.rules(), unit.owner, tile).is_some();
        let command = if adjacent && enemy {
            Command::Attack { unit: handle, target: tile }
        } else {
            Command::Kernel(KernelCommand::MarchTo { unit: handle, destination: tile })
        };
        self.order(command);
    }

    pub fn found_city(&mut self) {
        if let Some(settler) = self.selected {
            self.order(Command::Kernel(KernelCommand::FoundCity { settler }));
        }
    }

    pub fn order(&mut self, command: Command) {
        if self.ai_busy() {
            return;
        }
        self.message = self.client.submit(command).err().map(|e| e.to_string());
        if self.selected_unit().is_none_or(|u| u.moves_left == 0) {
            self.select_first_unit();
        }
    }

    // ---- cities ----

    /// Choose the own city on `tile` (a city label was clicked).
    pub fn select_city_on(&mut self, tile: u32) -> bool {
        let auth = self.client.game.view().auth;
        let city = auth.indexes().city_on_tile(TileId(tile));
        let own = city.filter(|c| auth.cities().get(*c).is_some_and(|c| c.owner == self.observer()));
        self.city = own.or(self.city);
        own.is_some()
    }

    pub fn city(&self) -> Option<&CityCore> {
        let city = self.city?;
        self.client.game.view().auth.cities().get(city).filter(|c| c.owner == self.observer())
    }

    /// What the chosen city may produce, with its cost.
    pub fn offers(&self) -> Vec<Offer> {
        let Some(city) = self.city else { return Vec::new() };
        let view = self.client.game.player_view(self.rules(), self.observer());
        let rules = self.rules();
        let units = rules.units.iter().enumerate().filter(|(_, u)| !u.unbuildable).map(|(i, u)| {
            (ProductionItem::Unit(kernel::ids::UnitKindId(i as u16)), u.name.clone())
        });
        let built = self.city().map(|c| c.buildings.clone()).unwrap_or_default();
        let buildings = rules
            .buildings
            .iter()
            .enumerate()
            .filter(|(i, b)| !b.unbuildable && !built.contains(&kernel::ids::BuildingKindId(*i as u16)))
            .map(|(i, b)| (ProductionItem::Building(kernel::ids::BuildingKindId(i as u16)), b.name.clone()));
        units
            .chain(buildings)
            .filter_map(|(item, name)| Some(Offer { cost: view.own_production_cost(city, item)?, item, name }))
            .collect()
    }

    /// The city builds `item` next (its queue replaced).
    pub fn produce(&mut self, item: ProductionItem) {
        let Some(city) = self.city else { return };
        let player = self.observer();
        self.order(Command::Kernel(KernelCommand::ClearProduction { player, city }));
        self.order(Command::Kernel(KernelCommand::EnqueueProduction { player, city, item }));
    }

    /// Food and production per turn of a city.
    pub fn city_rates(&self, city: CityHandle) -> (i32, i32) {
        let yields = self.client.game.derived().yields.of(city).unwrap_or_default();
        let population = self.client.game.view().auth.cities().get(city).map_or(0, |c| c.population);
        (yields.food - FOOD_PER_CITIZEN as i32 * population as i32, yields.production)
    }

    pub fn item_name(&self, item: ProductionItem) -> String {
        let rules = self.rules();
        match item {
            ProductionItem::Unit(kind) => rules.units.get(kind.0 as usize).map(|u| u.name.clone()),
            ProductionItem::Building(kind) => rules.buildings.get(kind.0 as usize).map(|b| b.name.clone()),
            ProductionItem::Project(_) => None,
        }
        .unwrap_or_default()
    }

    // ---- the HUD ----

    /// Food and production of the empire, and its cities.
    pub fn top_bar(&self) -> Vec<TopBarValue> {
        let auth = self.client.game.view().auth;
        let cities = auth.indexes().cities_of(self.observer()).to_vec();
        let (food, production) = cities.iter().map(|c| self.city_rates(*c)).fold((0, 0), |a, r| (a.0 + r.0, a.1 + r.1));
        let value = |icon, tint, text: String| TopBarValue { icon, tint, compact: text.clone(), text, tip: None, click: None, ready: false };
        vec![
            value(Icon::Apple, egui::Color32::from_rgb(140, 200, 90), tf!("sandbox.session.top-food", amount = signed(food))),
            value(Icon::Hammer, egui::Color32::from_rgb(214, 160, 90), tf!("sandbox.session.top-production", amount = signed(production))),
            value(Icon::City, egui::Color32::from_rgb(220, 220, 210), tf!("sandbox.session.top-cities", n = cities.len())),
        ]
    }

    /// Banners of the cities the observer sees: growth and production of its own.
    pub fn banners(&self) -> Vec<CityBanner> {
        let auth = self.client.game.view().auth;
        let markers = &self.client.render_world.cities;
        markers
            .iter()
            .filter_map(|marker| {
                let handle = auth.indexes().city_on_tile(marker.tile)?;
                let city = auth.cities().get(handle)?;
                let own = city.owner == self.observer();
                let strength = sandbox_rules::combat::city_strength(city) as i32;
                let (growth, production) = if own { self.city_progress(handle, city) } else { (None, None) };
                Some(CityBanner { tile: marker.tile.0, own, strength, hp: 0, max_hp: 0, can_strike: false, growth, production, tip: None, visitors: Vec::new() })
            })
            .collect()
    }

    #[allow(clippy::type_complexity)]
    fn city_progress(&self, handle: CityHandle, city: &CityCore) -> (Option<(u32, u32, Option<u32>)>, Option<(String, u32, u32, Option<u32>)>) {
        let (food, production) = self.city_rates(handle);
        let threshold = 10 + 5 * city.population;
        let stored = city.food_times100 / 100;
        let grow = (food > 0).then(|| threshold.saturating_sub(stored).div_ceil(food as u32));
        let view = self.client.game.player_view(self.rules(), self.observer());
        let head = city.queue.first().copied().and_then(|item| {
            let cost = view.own_production_cost(handle, item)?;
            let turns = (production > 0).then(|| cost.saturating_sub(city.production).div_ceil(production as u32));
            Some((self.item_name(item), city.production, cost, turns))
        });
        (Some((stored, threshold, grow)), head)
    }

    /// Unit names by kind, and whether each is a civilian.
    pub fn unit_label(&self, kind: u16) -> (String, bool) {
        let rules = self.rules();
        let name = rules.units.get(kind as usize).map(|u| u.name.clone()).unwrap_or_default();
        (name, strength(rules, kind) == 0)
    }
}
