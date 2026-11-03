//! Runtime derived values: city and player yields per channel (§17
//! platform/structure.md). Does not store population/buildings/ownership: that is auth.
//!
//! A city's yields (rules.md):
//!
//! ```text
//! base   worked tiles (derived::citizens, tile_yields) + specialists
//!        + the module's base terms (YieldTerms::city_base)
//! rate   City-scope modifiers by stage (rules::modifiers): flat (+ per
//!        citizen) → additive % (+ the module's empire percent) →
//!        multipliers → at least 0 → rounding; then the module's terms
//!        after the percents (YieldTerms::city_after)
//! ```
//!
//! The rules module adds its terms through [`YieldTerms`]
//! (`RulesModule::Yields`); a module change that moves them names the city
//! or player (`ModuleChange::stale_yields`). Everything the turn applies is
//! here. The v1 combined `Output` channel of legacy content counts as
//! production.
//!
//! rebuild is the oracle; apply is a local recompute of the affected cities.
//! Key perf property: unit movement does not touch yields (early return).

use crate::derived::citizens::{assign, CityWork, EmpireContext};
use crate::world::city::CITY_WORK_RADIUS;
use crate::effects::city::{city_modifiers, CityModifiers};
use crate::ids::{BuildingKindId, CityHandle, PlayerId, TileId};
use crate::math::hex_disc;
use crate::metrics::WorkCounters;
use crate::module::RulesModule;
use crate::rules::content::Yields;
use crate::rules::effects::YieldKind;
use crate::rules::modifiers::{Caps, StageTotals};
use crate::rules::vocabulary::Vocabulary;
use crate::rules::CompiledRuleset;
use crate::transaction::journal::{ChangeJournal, CityChange, PlayerChange, TileChange};
use crate::world::city::CityCore;
use crate::world::module::{ModuleChange, StaleYields};
use crate::world::state::AuthWorld;

/// Territory radius at founding: ring 1 (Classic).
pub const CITY_TERRITORY_RADIUS: i32 = 1;

/// The rules module's terms of a city's yields (`RulesModule::Yields`).
/// Every term is a pure function of the world: rebuild and the incremental
/// update see the same numbers.
pub trait YieldTerms<M: RulesModule> {
    /// What the module knows of a city's owner for one evaluation (Classic:
    /// the golden age and the unhappiness penalty of the turn).
    type Empire: Copy + std::fmt::Debug + Default + PartialEq + Eq;

    /// The least the city centre yields, before the clamp at 0 (2 food, 1 production).
    const CITY_MINIMUM: Yields = Yields {
        food: crate::derived::tile_yields::CITY_MIN_FOOD,
        production: crate::derived::tile_yields::CITY_MIN_PRODUCTION,
        ..Yields::ZERO
    };

    fn empire(auth: &AuthWorld<M>, player: PlayerId) -> Self::Empire;

    /// Added to a worked tile's yields before the centre minimum.
    fn tile_base(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore, tile: TileId) -> Yields;

    /// The finished yields of a worked tile.
    fn tile_final(empire: &Self::Empire, yields: &mut Yields);

    /// Added to the city's base (worked tiles and specialists).
    fn city_base(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore, modifiers: &CityModifiers<M>) -> Yields;

    /// The empire's additive percent on a channel.
    fn channel_percent(empire: &Self::Empire, kind: YieldKind<M>) -> i32;

    /// Added to the rates after the city's percents.
    fn city_after(auth: &AuthWorld<M>, city: CityHandle) -> Yields;

    /// Players whose every city a journal makes stale beyond the kernel's
    /// reasons (added with `add_once`).
    fn stale_players(changes: &ChangeJournal<M>, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, out: &mut Vec<PlayerId>);
}

/// The yield channels in a fixed order: every yield but the combined
/// `Output`, the module's yield last.
pub fn channels<M: RulesModule>() -> impl Iterator<Item = YieldKind<M>> {
    let kernel = [
        YieldKind::Food,
        YieldKind::Production,
        YieldKind::Gold,
        YieldKind::Science,
        YieldKind::Culture,
    ];
    let module = <M::Vocabulary as Vocabulary<M>>::YIELDS.iter().map(|y| YieldKind::Module(*y));
    kernel.into_iter().chain(module)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct YieldState {
    /// Rates per city (index = slot in CityStore; dead slots hold zero).
    pub city: Vec<Yields>,
    /// Sum of the cities per player (index = PlayerId.0).
    pub player: Vec<Yields>,
}

/// One city's yields for one evaluation.
#[derive(Debug, Clone)]
pub struct CityBreakdown<M: RulesModule> {
    /// Worked tiles and specialists.
    pub work: CityWork,
    /// Before modifiers: tiles + specialists + the module's base terms.
    pub base: Yields,
    /// After the city's modifiers and the empire's percents. Food is the
    /// gross food.
    pub rates: Yields,
    /// Every City-scope stage total (growth, costs, ...).
    pub modifiers: CityModifiers<M>,
    pub visited: u64,
    /// What `rates.production` is made of, for estimates toward other items.
    pub production: ProductionRate,
}

/// What a city's production rate is made of (rules.md): the rate
/// toward the head of the queue is `toward(item_production_percent)`; the
/// AI and the HUD put another item's percent in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProductionRate {
    /// Production of the worked tiles and specialists.
    pub base: i32,
    /// The city's production stages (the empire's percent included)
    /// without the percent toward the head of the queue.
    pub totals: StageTotals,
    /// Production added after the percents (`YieldTerms::city_after`).
    pub trade: i32,
}

impl ProductionRate {
    /// Production per turn toward an item with `item_percent`.
    pub fn toward(&self, item_percent: i32) -> i32 {
        let mut totals = self.totals;
        totals.add_percent(item_percent);
        totals.evaluate(self.base, Caps::at_least(0)) + self.trade
    }
}

/// The tiles a city works (centre first).
pub fn city_work<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore) -> CityWork {
    let modifiers = city_modifiers(auth, rules, core);
    assign(auth, rules, core, &EmpireContext::of(auth, core.owner), &modifiers.stats)
}

/// Yields of one city now (the empire context of its owner).
pub fn city_breakdown<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, core: &CityCore) -> CityBreakdown<M> {
    city_breakdown_with(auth, rules, core, &EmpireContext::of(auth, core.owner))
}

pub(crate) fn city_breakdown_with<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    core: &CityCore,
    empire: &EmpireContext<M>,
) -> CityBreakdown<M> {
    let modifiers = city_modifiers(auth, rules, core);
    let work = assign(auth, rules, core, empire, &modifiers.stats);
    let mut base = work.tile_yields.plus(work.specialist_yields);
    if work.specialist_count() > 0 {
        add_any_specialist_yields(&mut base, &modifiers.stats);
    }
    base = base.plus(M::Yields::city_base(auth, rules, core, &modifiers));
    let mut rates = Yields::default();
    let after = match auth.indexes().city_on_tile(core.tile) {
        Some(city) => M::Yields::city_after(auth, city),
        None => Yields::default(),
    };
    for kind in channels::<M>() {
        let mut totals = channel_totals(&modifiers, kind);
        totals.add_percent(M::Yields::channel_percent(&empire.module, kind));
        let value = totals.evaluate(base.of(kind), Caps::at_least(0));
        rates.add(kind, value + after.of(kind));
    }
    let production = production_rate(&modifiers, empire, base.production, after.production);
    debug_assert_eq!(production.toward(modifiers.item_production_percent), rates.production);
    CityBreakdown {
        work,
        base,
        rates,
        modifiers,
        visited: work.visited,
        production,
    }
}

/// The production stages without the percent toward the queue's head.
fn production_rate<M: RulesModule>(modifiers: &CityModifiers<M>, empire: &EmpireContext<M>, base: i32, trade: i32) -> ProductionRate {
    let mut totals = channel_totals(modifiers, YieldKind::Production);
    totals.add_percent(M::Yields::channel_percent(&empire.module, YieldKind::Production));
    totals.add_percent(-modifiers.item_production_percent);
    ProductionRate { base, totals, trade }
}

/// The flat yields of a city with at least one specialist
/// (`Stat::AnySpecialistYield`), once per city.
fn add_any_specialist_yields<M: RulesModule>(base: &mut Yields, stats: &crate::rules::stats::StatTotals<M>) {
    for (stat, amount) in stats.iter() {
        if let crate::rules::stats::Stat::AnySpecialistYield(kind) = stat {
            base.add(kind, amount);
        }
    }
}

/// Stage totals of one channel (legacy `Output` modifiers count as
/// production).
pub fn channel_totals<M: RulesModule>(modifiers: &CityModifiers<M>, kind: YieldKind<M>) -> StageTotals {
    let mut totals = modifiers.yields[kind.index()];
    if kind == YieldKind::Production {
        totals.merge(&modifiers.yields[YieldKind::<M>::Output.index()]);
    }
    totals
}

impl YieldState {
    pub fn rebuild<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Self {
        Self::rebuild_counted(auth, rules).0
    }

    pub fn rebuild_counted<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> (Self, WorkCounters) {
        let mut state = YieldState {
            city: vec![Yields::default(); auth.cities().capacity()],
            player: vec![Yields::default(); auth.players().count()],
        };
        let mut c = WorkCounters::default();
        for (h, core) in auth.cities().iter() {
            let b = city_breakdown(auth, rules, core);
            state.city[h.index as usize] = b.rates;
            if let Some(total) = state.player.get_mut(core.owner.0 as usize) {
                *total = total.plus(b.rates);
            }
            c.entities_visited += 1;
            c.tiles_visited += b.visited;
        }
        c.bytes_allocated = ((state.city.len() + state.player.len()) * 24) as u64;
        (state, c)
    }

    /// Rates of a city (None for a slot outside the store).
    pub fn of(&self, city: CityHandle) -> Option<Yields> {
        self.city.get(city.index as usize).copied()
    }

    /// Sum of a player's cities.
    pub fn of_player(&self, player: PlayerId) -> Yields {
        self.player
            .get(player.0 as usize)
            .copied()
            .unwrap_or_default()
    }

    pub fn apply_changes<M: RulesModule>(
        &mut self,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) {
        let _ = Self::apply_changes_counted(self, changes, auth, rules);
    }

    /// Local recompute. Returns counters for CI bounds.
    /// Invariant: move/unit changes do not touch yields (zero work).
    /// Owner-wide changes (technology, timed effects, owner-wide buildings)
    /// recompute every city of the player (O(player's cities)); a terrain
    /// change (lakes, coasts) rebuilds everything.
    pub fn apply_changes_counted<M: RulesModule>(
        ys: &mut YieldState,
        changes: &ChangeJournal<M>,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
    ) -> WorkCounters {
        let refresh = players_to_refresh(changes, auth, rules);
        let garrisons = garrison_cities(changes, auth, rules);
        let stale_cities: Vec<CityHandle> = changes
            .module_changes
            .iter()
            .filter_map(|change| match change.stale_yields() {
                StaleYields::City(city) => Some(city),
                StaleYields::Player(_) | StaleYields::Nothing => None,
            })
            .collect();
        if changes.city_changes.is_empty()
            && stale_cities.is_empty()
            && changes.tile_changes.is_empty()
            && refresh.is_empty()
            && garrisons.is_empty()
        {
            return WorkCounters::default();
        }
        let terrain = changes
            .tile_changes
            .iter()
            .any(|t| matches!(t, TileChange::TerrainSet { .. }));
        if terrain {
            let (fresh, c) = Self::rebuild_counted(auth, rules);
            *ys = fresh;
            return c;
        }
        let mut c = WorkCounters::default();
        ys.fit(auth);
        for change in &changes.city_changes {
            ys.apply_city_change(change, auth, rules, &mut c);
        }
        for city in stale_cities {
            ys.recompute(city, auth, rules, &mut c);
        }
        for change in &changes.tile_changes {
            ys.apply_tile_change(change, auth, rules, &mut c);
        }
        for city in garrisons {
            ys.recompute(city, auth, rules, &mut c);
        }
        for player in refresh {
            let list = auth.indexes().cities_of(player).to_vec();
            for h in list {
                ys.recompute(h, auth, rules, &mut c);
            }
        }
        c
    }

    fn fit<M: RulesModule>(&mut self, auth: &AuthWorld<M>) {
        let cities = auth.cities().capacity();
        if self.city.len() < cities {
            self.city.resize(cities, Yields::default());
        }
        let players = auth.players().count();
        if self.player.len() < players {
            self.player.resize(players, Yields::default());
        }
    }

    fn apply_city_change<M: RulesModule>(
        &mut self,
        change: &CityChange,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        c: &mut WorkCounters,
    ) {
        match change {
            CityChange::Founded(city)
            | CityChange::BuildingAdded { city, .. }
            | CityChange::PopulationChanged { city }
            | CityChange::WorkedChanged { city }
            | CityChange::StatusChanged { city } => self.recompute(*city, auth, rules, c),
            // A name changes no yield.
            CityChange::Renamed { .. } => {}
            CityChange::QueueChanged { city } => {
                // Only production percents toward the item read the queue.
                if rules.effect_index.has_production_percents {
                    self.recompute(*city, auth, rules, c);
                }
            }
            CityChange::Removed { city, owner, .. } => {
                // A razed city leaves its owner's total; the dead slot holds
                // nothing, as rebuild sees it.
                let idx = city.index as usize;
                let stale = self.city.get(idx).copied().unwrap_or_default();
                if let Some(total) = self.player.get_mut(owner.0 as usize) {
                    *total = total.minus(stale);
                }
                if let Some(slot) = self.city.get_mut(idx) {
                    *slot = Yields::default();
                }
            }
            CityChange::OwnerChanged { city, from, .. } => {
                // The old owner loses the city's whole contribution, then
                // the city counts for its new owner.
                let idx = city.index as usize;
                let stale = self.city[idx];
                if let Some(total) = self.player.get_mut(from.0 as usize) {
                    *total = total.minus(stale);
                }
                self.city[idx] = Yields::default();
                self.recompute(*city, auth, rules, c);
            }
        }
    }

    /// A tile change touches the cities that may work the tile (3 rings).
    fn apply_tile_change<M: RulesModule>(
        &mut self,
        change: &TileChange,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        c: &mut WorkCounters,
    ) {
        let tile = match change {
            TileChange::OwnerChanged { tile, .. }
            | TileChange::ImprovementBuilt { tile }
            | TileChange::ResourceSet { tile }
            | TileChange::RiverSet { tile }
            | TileChange::FeatureSet { tile }
            | TileChange::FeatureChopped { tile }
            | TileChange::NaturalWonderSet { tile }
            | TileChange::TerrainSet { tile } => *tile,
            // Roads do not change yields (the arm is explicit, not a wildcard).
            TileChange::RoadBuilt { .. } => return,
        };
        self.recompute_near(tile, auth, rules, c);
        if holds_wonder_resource(auth, rules, tile) {
            // A city has the resource on any tile of its territory, beyond
            // the worked rings (Marble speeds its wonders).
            self.recompute_territory(tile, auth, rules, c);
        }
    }

    /// Recompute the cities whose territory may hold `tile` beyond the
    /// worked rings (ring 4 and 5: the rings within 3 are done already).
    fn recompute_territory<M: RulesModule>(
        &mut self,
        tile: TileId,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        c: &mut WorkCounters,
    ) {
        let dims = auth.map().dimensions;
        for near in hex_disc(dims, tile, crate::world::city::MAX_TERRITORY_RADIUS) {
            c.tiles_visited += 1;
            if dims.distance(tile, near).is_some_and(|d| d <= CITY_WORK_RADIUS) {
                continue;
            }
            if let Some(city) = auth.indexes().city_on_tile(near) {
                self.recompute(city, auth, rules, c);
            }
        }
    }

    fn recompute_near<M: RulesModule>(
        &mut self,
        tile: TileId,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        c: &mut WorkCounters,
    ) {
        for near in hex_disc(auth.map().dimensions, tile, CITY_WORK_RADIUS) {
            c.tiles_visited += 1;
            if let Some(city) = auth.indexes().city_on_tile(near) {
                self.recompute(city, auth, rules, c);
            }
        }
    }

    /// Recompute one city; its owner's total moves by the delta (O(1)).
    fn recompute<M: RulesModule>(
        &mut self,
        city: CityHandle,
        auth: &AuthWorld<M>,
        rules: &CompiledRuleset<M>,
        c: &mut WorkCounters,
    ) {
        let Some(core) = auth.cities().get(city) else {
            return;
        };
        let b = city_breakdown(auth, rules, core);
        let idx = city.index as usize;
        let old = self.city[idx];
        self.city[idx] = b.rates;
        // Cities of players outside the store (a content bug) count for no
        // one, as in rebuild.
        if let Some(total) = self.player.get_mut(core.owner.0 as usize) {
            *total = total.minus(old).plus(b.rates);
        }
        c.entities_visited += 1;
        c.tiles_visited += b.visited;
    }
}

/// Players whose every city must be recomputed: a module change naming the
/// player (`StaleYields::Player`), a technology learned, timed effects
/// started/expired, a building with owner-wide effects built or its
/// city changing hands, a city changing faith (every religion's founder).
/// Deduplicated, first-seen order.
fn players_to_refresh<M: RulesModule>(
    changes: &ChangeJournal<M>,
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
) -> Vec<PlayerId> {
    let mut out = Vec::new();
    for change in &changes.module_changes {
        if let StaleYields::Player(player) = change.stale_yields() {
            add_once(&mut out, player);
        }
    }
    for pc in &changes.player_changes {
        if let Some(player) = owner_wide_player_change(pc) {
            add_once(&mut out, player);
        }
        // Effects gated by peace (Swords into Plowshares) move with a war.
        if !rules.effect_index.uses_at_peace {
            continue;
        }
        match pc {
            PlayerChange::WarDeclared { player, enemy } | PlayerChange::PeaceMade { player, enemy } => {
                add_once(&mut out, *player);
                add_once(&mut out, *enemy);
            }
            _ => {}
        }
    }
    // The module's reasons (Classic: a city changing faith moves its
    // religion's founder).
    M::Yields::stale_players(changes, auth, rules, &mut out);
    if rules.effect_index.has_capital_production {
        for player in capital_building_owners(changes, auth, rules) {
            add_once(&mut out, player);
        }
    }
    let owner_wide = &rules.effect_index.owner_wide_buildings;
    if owner_wide.is_empty() {
        return out;
    }
    for cc in &changes.city_changes {
        for player in owner_wide_city_change(cc, auth, owner_wide) {
            add_once(&mut out, player);
        }
    }
    out
}

/// Owners of capitals that got a building: their other cities' production
/// toward that building changes (Rome, `ProductionFilter::BuiltInCapital`).
fn capital_building_owners<M: RulesModule>(changes: &ChangeJournal<M>, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Vec<PlayerId> {
    let mut out = Vec::new();
    for change in &changes.city_changes {
        let CityChange::BuildingAdded { city, .. } = change else {
            continue;
        };
        let Some(core) = auth.cities().get(*city) else {
            continue;
        };
        if crate::world::city::is_capital(rules, core) {
            add_once(&mut out, core.owner);
        }
    }
    out
}

/// The tile has a resource that speeds wonders (Marble).
fn holds_wonder_resource<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, tile: TileId) -> bool {
    let wonder_resources = &rules.effect_index.wonder_resources;
    if wonder_resources.is_empty() {
        return false;
    }
    match auth.map().resource(tile).flatten() {
        Some(resource) => wonder_resources.contains(&resource),
        None => false,
    }
}

/// Cities a unit entered, left, appeared or died in, when a passive effect
/// depends on a garrison (`EffectIndex::has_garrison_condition`, Military
/// Caste): their garrison may have changed. Without such an effect unit
/// changes never touch yields.
fn garrison_cities<M: RulesModule>(changes: &ChangeJournal<M>, auth: &AuthWorld<M>, rules: &CompiledRuleset<M>) -> Vec<CityHandle> {
    use crate::transaction::journal::UnitChange;
    let mut out = Vec::new();
    if !rules.effect_index.has_garrison_condition {
        return out;
    }
    let mut add = |tile: TileId| {
        if let Some(city) = auth.indexes().city_on_tile(tile) {
            if !out.contains(&city) {
                out.push(city);
            }
        }
    };
    for change in &changes.unit_changes {
        match change {
            UnitChange::Moved { from, to, .. } => {
                add(*from);
                add(*to);
            }
            UnitChange::Created(unit) => {
                if let Some(view) = auth.units().get(*unit) {
                    add(view.core.tile);
                }
            }
            UnitChange::Destroyed { tile, .. } => add(*tile),
            UnitChange::Damaged { .. }
            | UnitChange::StanceChanged { .. }
            | UnitChange::Promoted { .. } => {}
        }
    }
    out
}

/// Add `player` unless it is in the list already.
pub fn add_once(players: &mut Vec<PlayerId>, player: PlayerId) {
    if !players.contains(&player) {
        players.push(player);
    }
}

fn owner_wide_player_change(change: &PlayerChange) -> Option<PlayerId> {
    match change {
        PlayerChange::TechnologyChosen { player }
        | PlayerChange::EffectsChanged { player } => Some(*player),
        PlayerChange::TurnEnded { .. }
        | PlayerChange::MapRevealed { .. }
        | PlayerChange::AreaRevealed { .. }
        | PlayerChange::TileNoticed { .. }
        | PlayerChange::WarDeclared { .. }
        | PlayerChange::PeaceMade { .. }
        | PlayerChange::BordersOpened { .. } => None,
    }
}

/// Players affected through owner-wide buildings: built, or their city
/// changed hands (both owners).
fn owner_wide_city_change<M: RulesModule>(
    change: &CityChange,
    auth: &AuthWorld<M>,
    owner_wide: &[BuildingKindId],
) -> Vec<PlayerId> {
    match change {
        CityChange::BuildingAdded { city, building } if owner_wide.contains(building) => auth
            .cities()
            .get(*city)
            .map(|core| core.owner)
            .into_iter()
            .collect(),
        CityChange::OwnerChanged { city, from, to } => {
            let Some(core) = auth.cities().get(*city) else {
                return Vec::new();
            };
            if core.buildings.iter().any(|b| owner_wide.contains(b)) {
                vec![*from, *to]
            } else {
                Vec::new()
            }
        }
        // A razed city may have held an owner-wide building.
        CityChange::Removed { owner, .. } => vec![*owner],
        CityChange::Founded(_)
        | CityChange::BuildingAdded { .. }
        | CityChange::PopulationChanged { .. }
        | CityChange::WorkedChanged { .. }
        | CityChange::StatusChanged { .. }
        | CityChange::QueueChanged { .. }
        | CityChange::Renamed { .. } => Vec::new(),
    }
}

