use crate::module::RulesModule;
use crate::world::module::ModuleCityOf;
use crate::ids::{BuildingKindId, CityHandle, EntityId, PlayerId, ProductionItem, TileId};

/// A city works tiles up to 3 rings away.
/// Unciv: models/ModConstants.kt (`cityWorkRange` 3), equal.
pub const CITY_WORK_RADIUS: i32 = 3;
/// Tiles in 3 rings, centre included.
pub const MAX_WORKED: usize = 37;
/// Specialist kinds a city tracks (Classic has 7; the compiler bounds it).
pub const MAX_SPECIALIST_KINDS: usize = 8;
/// Farthest tile a city's territory reaches.
/// Unciv: models/ModConstants.kt (`cityExpandRange` 5), equal.
pub const MAX_TERRITORY_RADIUS: i32 = 5;

/// Order queue limit (spam protection; Nation has none; we will raise it
/// when the UI shows the need).
pub const MAX_QUEUE: usize = 16;

/// Maximum culture growth radius (5 rings; rings 4-5 are
/// a buffer territory the city does not work, as in the rules).
/// Unciv: models/ModConstants.kt (`cityExpandRange` 5), equal.
pub const CULTURE_RADIUS_MAX: u8 = 5;

/// Radius for buying tiles with gold (Classic: only within 3 of the centre).
pub const PURCHASE_RADIUS_MAX: u8 = 3;

/// The AI strategy's rough estimate of a city's defense (base + citizens);
/// the combat value is `combat::city::city_strength`.
/// own value (no Unciv equivalent: an AI estimate).
pub const CITY_DEFENSE_BASE: i32 = 5;

/// Factor on the production cost inside the item price formula
/// `(30 P)^0.75` (the form is also Unciv's gold cost of an item; see
/// classic/docs/classic/SOURCES.md).
/// Unciv: models/ruleset/IConstruction.kt (`(30 cost)^0.75`), equal.
pub const GOLD_PER_PRODUCTION: u64 = 30;
/// Item prices are rounded down to a multiple of this.
/// Unciv: models/ruleset/IConstruction.kt (rounded down to 10), equal.
pub const PURCHASE_DIVISOR: u32 = 10;
/// Hit points of a city without buildings (walls, castles and the rest add
/// theirs, `crate::combat::city`). Own value, not compared with a public
/// source (unverified).
pub const CITY_BASE_HIT_POINTS: u16 = 200;

/// What a city is to its owner after a conquest (Classic annex / puppet /
/// raze). Track A's happiness reads `Occupied` and `Puppet`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CityStatus {
    /// Founded by its owner, or annexed and settled (courthouse).
    #[default]
    Own,
    /// Captured this turn: the conqueror decides (annex, puppet, raze,
    /// liberate). The end of the conqueror's turn makes it a puppet.
    AwaitingDecision,
    /// Annexed after a conquest (occupation unhappiness).
    Occupied,
    /// Puppet: governed automatically, cannot be ordered.
    Puppet,
    /// Burning down: one citizen less per turn of the owner, removed at 0.
    Razing,
}

/// Urgency surcharge (Nation): buying out costs x2 of the missing points.
/// Minimum 1: instant completion is never free, even with a full stockpile.
/// own value (no Unciv equivalent).
pub const HURRY_RATE: u32 = 2;

/// Gold price of an item of `cost` production with no percents of the
/// city (`gold_purchase_price` with none): what the AI expects to pay.
pub fn hurry_price(cost: u32, hurry_cost_modifier: i32) -> Option<u32> {
    gold_purchase_price(cost, hurry_cost_modifier, PurchasePercents::default())
}

/// The city's percents of a gold purchase (additive sums of `ModifyCost`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PurchasePercents {
    /// Every purchase (`CostTarget::Hurry`: Mercantilism,
    /// Big Ben).
    pub hurry: i32,
    /// Purchases of the item's kind (`UnitPurchase`: Mobilization;
    /// `BuildingPurchase`: Skyscrapers).
    pub kind: i32,
}

/// Gold price of an item of `cost` production (rules.md):
/// ⌊(30·P)^0.75⌋ — in integers ⌊((30·P)³)^¼⌋ — × (100 + hurry %), × (100 +
/// the item's HurryCostModifier) %, × (100 + the kind's purchase %), each
/// truncated, then rounded down to 10. The production already invested does
/// not lower it. None: not for sale (HurryCostModifier −1: wonders).
pub fn gold_purchase_price(cost: u32, hurry_cost_modifier: i32, percents: PurchasePercents) -> Option<u32> {
    if hurry_cost_modifier == -1 {
        return None;
    }
    let x = u64::from(cost) * GOLD_PER_PRODUCTION;
    let mut price = x.saturating_mul(x).saturating_mul(x).isqrt().isqrt() as i64;
    for percent in [percents.hurry, hurry_cost_modifier, percents.kind] {
        price = price * (100 + i64::from(percent.max(-100))) / 100;
    }
    let price = price.clamp(0, i64::from(u32::MAX)) as u32;
    Some(price / PURCHASE_DIVISOR * PURCHASE_DIVISOR)
}

/// Cost of the city's next territory tile (border growth):
/// `round(6 (t + 1.4813)^1.3)`, t is how many tiles the city has already
/// claimed beyond its first ring (by culture and purchase together). Unciv:
/// CityExpansionManager.getCultureToNextTile, equal (Standard speed, no
/// modifiers). The first claim costs 10.
///
/// A table instead of `powf` at runtime: bit-exact `f64::powf` is not
/// guaranteed to match across platforms, and replay determinism (§25)
/// does not forgive that: a ULP mismatch right at the threshold would break the hash.
/// The test `culture_cost_table_matches_formula` checks every value against
/// the formula, so the derivation is single.
pub const CULTURE_COST_LEN: usize = 128;

pub static CULTURE_COSTS: [u32; CULTURE_COST_LEN] = [
    10, 20, 30, 42, 55, 68, 82, 97, 112, 127, 143, 160, 177, 194, 211, 229, 247, 266, 285, 304, 323,
    343, 363, 383, 404, 425, 446, 467, 488, 510, 532, 554, 576, 598, 621, 644, 667, 690, 714, 737,
    761, 785, 809, 833, 858, 882, 907, 932, 957, 982, 1008, 1033, 1059, 1085, 1111, 1137, 1163,
    1189, 1216, 1242, 1269, 1296, 1323, 1350, 1378, 1405, 1432, 1460, 1488, 1516, 1544, 1572, 1600,
    1629, 1657, 1686, 1714, 1743, 1772, 1801, 1830, 1860, 1889, 1918, 1948, 1978, 2007, 2037, 2067,
    2097, 2128, 2158, 2188, 2219, 2249, 2280, 2311, 2342, 2373, 2404, 2435, 2466, 2497, 2529, 2560,
    2592, 2624, 2655, 2687, 2719, 2751, 2783, 2815, 2848, 2880, 2913, 2945, 2978, 3011, 3043, 3076,
    3109, 3142, 3175, 3209, 3242, 3275, 3309,
];

/// Cost of the next culture claim (a pure function: one for
/// produce_city, the HUD and tests). Beyond the table: the last value
/// (unreachable: a city cannot acquire more than 91 tiles of ring 5).
pub fn culture_cost(acquired: u32) -> u32 {
    CULTURE_COSTS[(acquired as usize).min(CULTURE_COST_LEN - 1)]
}

/// Game length for GameProgress of the legacy tile price `purchase_price`
/// (Standard speed, 500 turns; only tests read that price: the game's tile
/// price is `territory::tile_purchase_offer`, its length `SpeedValues::max_rounds`).
/// Unciv: Speeds.json (Standard, 500 turns), equal.
pub const GAME_MAX_TURNS: u32 = 500;

/// Gold price of a tile (Classic): `floor5(base·(1 + 4g))`,
/// where `g = min(turn, 500)/500` is GameProgress (the share of the game played).
/// base: ring 2 is 50 gold, ring 3 is 75, growing with the share of the game played (own value,
/// not compared with Unciv's `getGoldCostOfTile`, which differs: unverified).
/// Ring 1 is 25 (a defensive case: ring 1 is given at founding).
/// All arithmetic is integer: determinism without f64.
pub fn purchase_price(ring: u8, turn: u32) -> u32 {
    let base = match ring {
        0 | 1 => 25u32,
        2 => 50,
        3 => 75,
        // Buying further is not allowed (validate rejects earlier); early return,
        // otherwise u32::MAX overflows in the formula in debug.
        _ => return u32::MAX,
    };
    let t = turn.min(GAME_MAX_TURNS);
    let raw = base * (GAME_MAX_TURNS + 4 * t) / GAME_MAX_TURNS;
    raw - raw % 5
}

/// A city name as the uniqueness rule compares it.
fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The names every living city holds. A captured city keeps its name and so keeps it reserved.
fn names_in_use<M: RulesModule>(auth: &crate::world::state::AuthWorld<M>) -> std::collections::HashSet<String> {
    auth.cities().iter().map(|(_, city)| name_key(&city.name)).collect()
}

/// The names the owner's nation offers its cities, in the order they are given out.
/// Empty for a player the module names its cities itself (a city-state) and for a nation
/// without a list.
pub fn city_name_pool<'r, M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &'r crate::rules::CompiledRuleset<M>,
    owner: PlayerId,
) -> &'r [String] {
    let module_named = auth.players().kind_of(owner).module().is_some_and(|kind| {
        crate::world::player::ModulePlayerKind::city_name(kind, rules).is_some()
    });
    match rules.nations.get(auth.players().nation_of(owner) as usize) {
        Some(nation) if !module_named => &nation.city_names,
        _ => &[],
    }
}

/// The pool names no city holds: the choices of a founder or of a rename, with their place in
/// the pool.
pub fn free_city_names<'r, M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &'r crate::rules::CompiledRuleset<M>,
    owner: PlayerId,
) -> Vec<(u16, &'r str)> {
    let used = names_in_use(auth);
    let pool = city_name_pool(auth, rules, owner);
    (0..pool.len())
        .filter(|&at| !used.contains(&name_key(&pool[at])))
        .map(|at| (at as u16, pool[at].as_str()))
        .collect()
}

/// Pick a name only when founding: the first pool name no city holds, else the pool names
/// numbered from 2 ("Name 2"); a nation without a pool numbers its own name. Captured
/// cities keep and reserve their names, as do other nations' and renamed ones. No random
/// stream or per-player counter is needed; save/load therefore preserves the next choice.
pub fn next_city_name<M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &crate::rules::CompiledRuleset<M>,
    owner: PlayerId,
) -> String {
    let used = names_in_use(auth);
    let nation = rules.nations.get(auth.players().nation_of(owner) as usize);
    let kind_name = auth.players().kind_of(owner).module().and_then(|kind| {
        crate::world::player::ModulePlayerKind::city_name(kind, rules)
    });
    let fallback = match &kind_name {
        Some(name) => name.clone(),
        None => nation.map(|c| format!("{} Settlement", c.name.trim())).unwrap_or_else(|| "Settlement".into()),
    };
    let pool = city_name_pool(auth, rules, owner);
    let bases: Vec<&str> = if pool.is_empty() { vec![fallback.as_str()] } else { pool.iter().map(String::as_str).collect() };
    for base in &bases {
        if !used.contains(&name_key(base)) { return (*base).into(); }
    }
    for suffix in 2_u64.. {
        for base in &bases {
            let candidate = format!("{base} {suffix}");
            if !used.contains(&name_key(&candidate)) { return candidate; }
        }
    }
    unreachable!("a finite live-city set cannot exhaust the name space")
}

#[derive(Debug, Clone)]
pub struct CityCore {
    /// Persistent identity (survives save/load; handles do not).
    pub id: EntityId,
    pub tile: TileId,
    pub owner: PlayerId,
    pub name: String,
    pub population: u32,
    /// Food in store, in hundredths: growth at
    /// the threshold, starvation below zero (systems::growth).
    pub food_times100: u32,
    pub buildings: Vec<BuildingKindId>,
    /// Accumulated production points (authoritative truth: persisted, hashed).
    pub production: u32,
    /// Order queue (Nation): the head is built first, overflow goes after.
    pub queue: Vec<ProductionItem>,
    /// Ring tiles taken off work by hand (toggle). The rest is
    /// the auto-pick of the best by (F+P, tile). Deterministic: stored sorted.
    pub unworked: Vec<TileId>,
    /// Accumulated culture (bank; spent on tiles, as in Classic).
    pub culture: u32,
    /// Culture level: border growths paid with
    /// culture, the argument of the border threshold. Purchases do not
    /// raise it. Persisted, hashed.
    pub acquired: u16,
    /// Maximum ring of acquired tiles (1 at founding).
    /// Capturing a city claims a disc of this radius.
    pub radius: u8,
    /// Citizen orders: focus, locked tiles, hand-assigned specialists.
    pub citizens: crate::world::citizens::CitizenPlan,
    /// Food kept and great person points.
    pub economy: crate::world::city_economy::CityEconomy,
    /// Damage taken (Classic stores damage): HP = max HP − damage.
    pub damage: u16,
    /// The player who founded the city.
    pub original_owner: PlayerId,
    /// The first city of a major nation: its original capital
    /// (domination victory, cannot be razed).
    pub original_capital: bool,
    pub status: CityStatus,
    /// Turns of resistance after a conquest (no production, no growth).
    pub resistance_turns: u16,
    /// The city made its ranged strike this turn.
    pub strike_made: bool,
    /// Connected to its owner's capital at the start of the owner's turn
    /// (the capital itself counts as connected). Kept only when the ruleset
    /// asks (`EffectIndex::uses_capital_connection`, Messenger of the
    /// Gods); Classic refreshes it once a turn too.
    pub capital_connection: CapitalConnection,
    /// Round the owner founded or acquired it:
    /// a city held longer yields more gold to its conqueror.
    pub acquired_round: u32,
}

/// Whether a city reaches its owner's capital over roads or harbors
/// (`derived::connections`), as of the start of the owner's turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CapitalConnection {
    #[default]
    Disconnected,
    Connected,
}

impl CityCore {
    /// A fresh city of `owner` (founding and harness) in `round`.
    pub fn founded(
        id: EntityId,
        tile: TileId,
        owner: PlayerId,
        name: String,
        original_capital: bool,
        round: u32,
    ) -> Self {
        Self {
            id,
            tile,
            owner,
            name,
            population: 1,
            food_times100: 0,
            buildings: vec![],
            production: 0,
            queue: vec![],
            unworked: vec![],
            culture: 0,
            acquired: 0,
            radius: 1,
            damage: 0,
            original_owner: owner,
            original_capital,
            status: CityStatus::Own,
            resistance_turns: 0,
            strike_made: false,
            capital_connection: CapitalConnection::Disconnected,
            acquired_round: round,
            citizens: Default::default(),
            economy: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
struct CitySlot<M: RulesModule> {
    core: Option<CityCore>,
    /// The rules module's state of the city (`ModuleCity`).
    module: ModuleCityOf<M>,
    generation: u32,
}

/// Cities in slots. A removed (razed) city leaves its slot empty for good:
/// slots are never reused, so slot order stays entity order.
#[derive(Debug, Clone, Default)]
pub struct CityStore<M: RulesModule> {
    slots: Vec<CitySlot<M>>,
    live: usize,
}

impl<M: RulesModule> CityStore<M> {
    /// The rules module's state of a city (None: no such city).
    pub fn module_of(&self, h: CityHandle) -> Option<&ModuleCityOf<M>> {
        let slot = self.slots.get(h.index as usize)?;
        (slot.generation == h.generation && slot.core.is_some()).then_some(&slot.module)
    }

    /// The rules module's state of a city, for its systems.
    pub fn module_mut(&mut self, h: CityHandle) -> Option<&mut ModuleCityOf<M>> {
        let slot = self.slots.get_mut(h.index as usize)?;
        (slot.generation == h.generation && slot.core.is_some()).then_some(&mut slot.module)
    }

    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, h: CityHandle) -> Option<&CityCore> {
        self.slots.get(h.index as usize).and_then(|s| {
            if s.generation == h.generation {
                s.core.as_ref()
            } else {
                None
            }
        })
    }

    /// Number of slots (including free ones), to align the derived vecs.
    pub fn capacity(&self) -> usize {
        self.slots.len()
    }

    /// Cities with handles, ascending entity id: cities are never removed
    /// and are inserted in id order, so slot order is entity order (the
    /// validator checks it).
    pub fn iter(&self) -> impl Iterator<Item = (CityHandle, &CityCore)> {
        self.slots.iter().enumerate().filter_map(|(i, s)| {
            s.core.as_ref().map(|core| {
                (
                    CityHandle {
                        index: i as u32,
                        generation: s.generation,
                    },
                    core,
                )
            })
        })
    }

    /// The live city of persistent id `id` (a command journal names cities by id).
    pub fn handle_of(&self, id: EntityId) -> Option<CityHandle> {
        self.iter().find(|(_, core)| core.id == id).map(|(handle, _)| handle)
    }

    /// A new city (the newest entity): always a new slot.
    pub fn insert(&mut self, core: CityCore) -> CityHandle {
        let index = self.slots.len() as u32;
        self.slots.push(CitySlot {
            core: Some(core),
            module: ModuleCityOf::<M>::default(),
            generation: 0,
        });
        self.live += 1;
        CityHandle {
            index,
            generation: 0,
        }
    }

    /// Remove a city (razed): the slot stays empty, its handle goes stale.
    pub fn remove(&mut self, h: CityHandle) -> Option<CityCore> {
        let slot = self.slots.get_mut(h.index as usize)?;
        if slot.generation != h.generation {
            return None;
        }
        let core = slot.core.take()?;
        slot.module = ModuleCityOf::<M>::default();
        slot.generation += 1;
        self.live -= 1;
        Some(core)
    }

    /// Mutable access for the systems of the transaction.
    pub fn get_mut(&mut self, h: CityHandle) -> Option<&mut CityCore> {
        let slot = self.slots.get_mut(h.index as usize)?;
        if slot.generation != h.generation {
            return None;
        }
        slot.core.as_mut()
    }

    /// Add a building. The outcome is a type, not a bool: a stale handle
    /// is distinguishable from success (the bool hid it).
    pub fn add_building(&mut self, h: CityHandle, b: BuildingKindId) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.buildings.push(b);
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// Accumulate production.
    pub fn add_production(&mut self, h: CityHandle, amount: u32) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.production = core.production.saturating_add(amount);
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// Write off a cost. Three outcomes instead of a bool: "not enough" and "the city
    /// is gone" are different states, the caller reacts to them differently.
    pub(crate) fn spend_production(&mut self, h: CityHandle, cost: u32) -> SpendOutcome {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) if core.production >= cost => {
                    core.production -= cost;
                    SpendOutcome::Spent
                }
                Some(_) => SpendOutcome::NotEnough,
                None => SpendOutcome::TargetGone,
            },
            _ => SpendOutcome::TargetGone,
        }
    }

    /// Put an order at the tail of the queue. Overflow is an honest refusal
    /// (the UI shows the limit), not a silent drop.
    pub fn enqueue(&mut self, h: CityHandle, item: ProductionItem) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) if core.queue.len() < MAX_QUEUE => {
                    core.queue.push(item);
                    CityWrite::Applied
                }
                Some(_) => CityWrite::QueueFull,
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// Clear the queue (changing the order the Nation way: clear first).
    pub(crate) fn clear_queue(&mut self, h: CityHandle) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.queue.clear();
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// Food into the store (whole food; the store keeps hundredths).
    pub fn add_food(&mut self, h: CityHandle, amount: u32) -> CityWrite {
        match self.get_mut(h) {
            Some(core) => {
                core.food_times100 = core.food_times100.saturating_add(amount.saturating_mul(100));
                CityWrite::Applied
            }
            None => CityWrite::TargetGone,
        }
    }

    /// Pop the queue head (a completed order goes to work).
    pub(crate) fn pop_queue(&mut self, h: CityHandle) -> Option<ProductionItem> {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) if !core.queue.is_empty() => Some(core.queue.remove(0)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Accumulate culture (saturating).
    pub fn add_culture(&mut self, h: CityHandle, amount: u32) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.culture = core.culture.saturating_add(amount);
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// The culture level grows: spend
    /// the threshold, +1 level, radius = max(radius, ring) when a tile is
    /// taken. The caller checked the threshold and the tile.
    pub(crate) fn raise_culture_level(
        &mut self,
        h: CityHandle,
        cost: u32,
        ring: Option<u8>,
    ) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.culture = core.culture.saturating_sub(cost);
                    core.acquired = core.acquired.saturating_add(1);
                    if let Some(ring) = ring {
                        core.radius = core.radius.max(ring);
                    }
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// A tile bought with gold: radius = max(radius, ring); the culture
    /// level does not change (only culture raises it).
    pub fn extend_radius(&mut self, h: CityHandle, ring: u8) -> CityWrite {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    core.radius = core.radius.max(ring);
                    CityWrite::Applied
                }
                None => CityWrite::TargetGone,
            },
            _ => CityWrite::TargetGone,
        }
    }

    /// Owner change on capture (the territory is moved by the caller's claim).
    pub fn set_owner(&mut self, h: CityHandle, owner: PlayerId) -> CityWrite {
        match self.get_mut(h) {
            Some(core) => {
                core.owner = owner;
                CityWrite::Applied
            }
            None => CityWrite::TargetGone,
        }
    }

    /// Toggle a manual tile: take it off work / return it to the auto-pick.
    /// Returns the new state (true = now worked).
    /// The order of unworked is sorted by tile (deterministic).
    pub(crate) fn toggle_worked(&mut self, h: CityHandle, tile: TileId) -> Option<bool> {
        match self.slots.get_mut(h.index as usize) {
            Some(slot) if slot.generation == h.generation => match slot.core.as_mut() {
                Some(core) => {
                    if let Some(pos) = core.unworked.iter().position(|t| *t == tile) {
                        core.unworked.remove(pos);
                        Some(true)
                    } else {
                        core.unworked.push(tile);
                        core.unworked.sort_by_key(|t| t.0);
                        Some(false)
                    }
                }
                None => None,
            },
            _ => None,
        }
    }
}

/// Outcome of a city mutation by handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CityWrite {
    /// Applied to a live city.
    Applied,
    /// Stale handle: the city was razed or the slot reused.
    TargetGone,
    /// Queue full (enqueue only): the order was not taken, the world is untouched.
    QueueFull,
}

/// Outcome of a production write-off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendOutcome {
    /// Written off, can build.
    Spent,
    /// Not enough money (the city is alive, keep accumulating).
    NotEnough,
    /// Stale handle.
    TargetGone,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn culture_cost_table_matches_formula() {
        // The table is derived from the border-tile culture formula (`derived::culture`);
        // a mismatch = a generation bug.
        for (t, &actual) in CULTURE_COSTS.iter().enumerate() {
            let v = 6.0 * (t as f64 + 1.4813).powf(1.3);
            let expect = v.round() as u32;
            assert_eq!(actual, expect, "t={t}");
        }
        assert_eq!(culture_cost(0), 10, "first tile costs 10");
        assert_eq!(
            culture_cost(u32::MAX),
            *CULTURE_COSTS.last().unwrap(),
            "beyond table saturates"
        );
    }

    #[test]
    fn purchase_price_follows_classic_game_progress() {
        // g = 0: base 50/75.
        assert_eq!(purchase_price(2, 0), 50);
        assert_eq!(purchase_price(3, 0), 75);
        // g = 1 (end of the game, Standard 500 turns): x5 -> 250/375.
        assert_eq!(purchase_price(2, GAME_MAX_TURNS), 250);
        assert_eq!(purchase_price(3, GAME_MAX_TURNS), 375);
        // g = 0.5: 50·(1+2) = 150.
        assert_eq!(purchase_price(2, GAME_MAX_TURNS / 2), 150);
        // A turn past 500 is clamped (a time victory may not be enabled).
        assert_eq!(purchase_price(2, 9_999), 250);
        // Rings beyond 3 cannot be bought.
        assert_eq!(purchase_price(4, 0), u32::MAX);
        // The price never falls.
        let mut prev = 0;
        for turn in (0..=GAME_MAX_TURNS).step_by(10) {
            let p = purchase_price(2, turn);
            assert!(p >= prev, "price fell at turn {turn}: {p} < {prev}");
            prev = p;
        }
    }
}


/// The city holds the palace: it is its owner's capital.
pub fn is_capital<M: RulesModule>(rules: &crate::rules::CompiledRuleset<M>, core: &CityCore) -> bool {
    core.buildings.iter().any(|b| {
        rules
            .buildings
            .get(b.0 as usize)
            .map(|d| d.kind == crate::rules::content::BuildingKind::Palace)
            .unwrap_or(false)
    })
}

/// The player's capital (None without a palace).
pub fn capital_of<M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &crate::rules::CompiledRuleset<M>,
    player: PlayerId,
) -> Option<CityHandle> {
    for handle in auth.indexes().cities_of(player) {
        let Some(core) = auth.cities().get(*handle) else {
            continue;
        };
        if is_capital(rules, core) {
            return Some(*handle);
        }
    }
    None
}

/// The garrison: the owner's best land combat unit on the city tile.
pub fn garrison<M: RulesModule>(
    auth: &crate::world::state::AuthWorld<M>,
    rules: &crate::rules::CompiledRuleset<M>,
    city: &CityCore,
) -> Option<crate::ids::UnitHandle> {
    let mut best: Option<(u16, crate::ids::UnitHandle)> = None;
    for &unit in auth.indexes().units_on(city.tile) {
        let Some(view) = auth.units().get(unit) else {
            continue;
        };
        let core = view.core;
        if core.owner != city.owner || rules.is_civilian(core.kind) {
            continue;
        }
        let def = rules.units.get(core.kind.0 as usize);
        if def.is_some_and(|d| d.domain != crate::rules::content::Domain::Land || d.strength == 0) {
            continue;
        }
        let strength = def
            .map(|d| d.strength)
            .unwrap_or(crate::rules::compiled::DEFAULT_STRENGTH);
        if best.map(|(s, _)| strength > s).unwrap_or(true) {
            best = Some((strength, unit));
        }
    }
    best.map(|(_, unit)| unit)
}
