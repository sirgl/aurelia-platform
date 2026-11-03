//! Reproducible artificial worlds for tests/benches/profiling (§27).

use crate::ids::{PlayerId, TileId, UnitKindId};
use crate::module::RulesModule;
use crate::math::{HexCoord, HEX_DIRS};
use crate::rules::content::{StartRegion, StartRegions};
use crate::rules::roles::{Climate, FeatureRole, Ground, StartingUnit};
use crate::rules::source::StartBias;
use crate::rules::CompiledRuleset;
use super::landscape;
use crate::world::builder::WorldBuilder;
use crate::world::state::{GameConfig, GameState};
use crate::scenario::map_script::MapScript;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct ScenarioConfig {
    pub seed: u64,
    pub players: u8,
    pub width: u32,
    pub height: u32,
    pub cities: usize,
    pub units: usize,
    /// Share of water terrain in percent (0..=100). Water is impassable (navigation::cost).
    pub water_ratio: u8,
}

impl ScenarioConfig {
    pub fn representative_late_game() -> Self {
        Self {
            seed: 1,
            players: 8,
            width: 200,
            height: 125, // 25k tiles, the middle of 20k-50k from §2.1
            cities: 200, // 100–300
            units: 1000, // 500–1500
            water_ratio: 15,
        }
    }

    pub fn stress() -> Self {
        Self {
            seed: 2,
            players: 8,
            width: 224,
            height: 224, // ~50k
            cities: 500,
            units: 5000,
            water_ratio: 15,
        }
    }

    /// Quick game from the menu: 6 players, compact 64x64, 10% water and
    /// a deterministic seed: the same start on every run (as in
    /// Nation: a small map, sensible placements, no late-game noise).
    pub fn quick_game() -> Self {
        Self {
            seed: 20_260_928, // a date, not random: session determinism
            players: 6,
            width: 64,
            height: 64,
            cities: 0,
            units: 0,
            water_ratio: 10,
        }
    }
}

/// Deterministic xorshift64: no external deps, no wall-clock in the simulation.
struct XorShift64(u64);

impl XorShift64 {
    fn next(&mut self) -> u64 {
        let mut x = self.0.max(1);
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

pub fn generate<M: RulesModule>(cfg: &ScenarioConfig, rules: &CompiledRuleset<M>) -> GameState<M> {
    generate_scripted(cfg, rules, MapScript::Fractal)
}

/// `generate` with the land shaped by a map script (Fractal is `generate`).
pub fn generate_scripted<M: RulesModule>(cfg: &ScenarioConfig, rules: &CompiledRuleset<M>, script: MapScript) -> GameState<M> {
    generate_scripted_builder(cfg, rules, script).build()
}

/// `generate_scripted`, the world still under construction: the caller places
/// the starts, city-states and the rest, then builds.
pub fn generate_scripted_builder<'r, M: RulesModule>(
    cfg: &ScenarioConfig,
    rules: &'r CompiledRuleset<M>,
    script: MapScript,
) -> WorldBuilder<'r, M> {
    generate_scripted_builder_with(cfg, rules, script, landscape::LandOptions::CLASSIC)
}

/// `generate_scripted_builder` with the land's shares of `land`
/// (`LandOptions::CLASSIC` is `generate_scripted_builder`, bit for bit).
pub fn generate_scripted_builder_with<'r, M: RulesModule>(
    cfg: &ScenarioConfig,
    rules: &'r CompiledRuleset<M>,
    script: MapScript,
    land_options: landscape::LandOptions,
) -> WorldBuilder<'r, M> {
    let mut game = WorldBuilder::new(
        rules,
        GameConfig {
            players: cfg.players,
            width: cfg.width,
            height: cfg.height,
            rng_seed: cfg.seed,
        },
    );
    let tiles = (cfg.width as u64) * (cfg.height as u64);
    let mut rng = XorShift64(cfg.seed);
    // The land (scenario/landscape.rs): plot types and climate from noise
    // fields of the seed, rivers from the map RNG, then features and
    // natural wonders. One journal per layer: derived state rebuilds once.
    let mut land = landscape::land_shaped_with(cfg.width, cfg.height, cfg.water_ratio, cfg.seed, script, land_options);
    let edges = landscape::rivers(&land, |n| rng.below(n));
    let river = landscape::river_tiles(&land, &edges);
    let mut features = landscape::features(&land, &river, rules);
    let wonders = landscape::natural_wonders(&mut land, &mut features, &river, rules);
    let batch = landscape::terrain_batch(&land, rules.terrain_kinds.palette());
    game.set_terrain(&batch);
    game.set_rivers(&edges);
    let placed: Vec<(TileId, crate::ids::FeatureId)> = features
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.map(|f| (TileId(i as u32), f)))
        .collect();
    game.set_features(&placed);
    game.set_natural_wonders(&wonders);
    // Resources by content: for each kind, on its terrains with its density
    // (roll order fixed: kinds, then tiles row-major). The land loop never
    // takes water or a natural wonder; the sea is placed after it.
    {
        let mut placements: Vec<(TileId, crate::ids::ResourceId)> = Vec::new();
        for (ri, def) in rules.resources.iter().enumerate() {
            for (i, (_, terrain)) in batch.iter().enumerate() {
                if terrain.0 == rules.terrain_kinds.palette().code(Ground::Water) {
                    continue;
                }
                if !def.terrains.is_empty() && !def.terrains.contains(&terrain.0) {
                    continue;
                }
                let density = if land_options.resource_percent == 100 { u64::from(def.density) } else { u64::from(def.density) * u64::from(land_options.resource_percent) / 100 };
                if rng.below(100) < density && !wonders.iter().any(|w| w.0 .0 == i as u32) {
                    placements.push((TileId(i as u32), crate::ids::ResourceId(ri as u16)));
                }
            }
        }
        // The sea after the land, from the hashes of its own (`sea_resources`),
        // so that nothing the land drew from `rng` moves.
        placements.extend(super::sea_resources::place(&land, &features, &wonders, rules, cfg.seed));
        game.place_resources(&placements);
    }

    for i in 0..cfg.cities {
        let owner = PlayerId((rng.below(cfg.players as u64)) as u8);
        let tile = TileId(rng.below(tiles) as u32);
        let _ = i;
        // One city per tile: a second draw on an occupied tile is dropped
        // (the RNG stream stays the same for the rest of the world).
        if game.state().view().auth.indexes().city_on_tile(tile).is_some() {
            continue;
        }
        let _ = game.found_city(owner, tile);
    }
    for i in 0..cfg.units {
        let owner = PlayerId((rng.below(cfg.players as u64)) as u8);
        let tile = TileId(rng.below(tiles) as u32);
        // Alternate kinds by ROLE, not by index: with content, settlers
        // and starting warriors (fixture fights are fair); adding new
        // units must not change the starting spawn. Without content the kind
        // is ignored by fallbacks (0/1 as before). Deterministic.
        let kind = if i % 2 == 0 {
            settler_kind(rules)
        } else {
            rules.roles.starting_unit(StartingUnit::Defense).unwrap_or(UnitKindId(1))
        };
        let _ = game.spawn_unit(owner, tile, kind);
    }
    game
}

/// Settler kind from content (the first founder, not an index). Fallback kind 0
/// for empty test content (there is nobody to check a founder but the tests).
pub fn settler_kind<M: RulesModule>(rules: &CompiledRuleset<M>) -> UnitKindId {
    rules.first_founder().unwrap_or(UnitKindId(0))
}

/// New game: an empty map, one settler per player, starts picked
/// fairly (land, margin, spread, quality, nation bias). Nations are by seed,
/// P0 gets the first of the shuffled pool (no menu: web/tests/examples).
/// explored is empty (except the start surroundings, which the spawn lights up),
/// turn 0. Unlike generate() (a late-game fixture for perf).
pub fn generate_new_game<M: RulesModule>(rules: &CompiledRuleset<M>, cfg: &ScenarioConfig) -> GameState<M> {
    generate_new_game_builder(rules, cfg).build()
}

/// `generate_new_game`, the world still under construction (a module that
/// adds its own setup, as the sandbox's wars, builds after it).
pub fn generate_new_game_builder<'r, M: RulesModule>(rules: &'r CompiledRuleset<M>, cfg: &ScenarioConfig) -> WorldBuilder<'r, M> {
    generate_player_game_inner(rules, cfg, None, MapScript::Fractal)
}

/// A game with a chosen human nation: P0 = human_nation, the rest are unique
/// (seed-deterministic). The starts are the same fair ones as in generate_new_game.
pub fn generate_player_game<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    cfg: &ScenarioConfig,
    human_nation: u8,
) -> GameState<M> {
    generate_player_game_inner(rules, cfg, Some(human_nation), MapScript::Fractal).build()
}

/// `generate_player_game` on the land of a map script (`cfg.water_ratio`
/// is the final water share: see `MapScript::water_ratio`).
pub fn generate_player_game_scripted<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    cfg: &ScenarioConfig,
    human_nation: u8,
    script: MapScript,
) -> GameState<M> {
    generate_player_game_inner(rules, cfg, Some(human_nation), script).build()
}

fn generate_player_game_inner<'r, M: RulesModule>(
    rules: &'r CompiledRuleset<M>,
    cfg: &ScenarioConfig,
    human_nation: Option<u8>,
    script: MapScript,
) -> WorldBuilder<'r, M> {
    let empty = ScenarioConfig {
        cities: 0,
        units: 0,
        ..cfg.clone()
    };
    let mut game = generate_scripted_builder(&empty, rules, script);
    let nations = assign_nations(rules, cfg.players, human_nation, cfg.seed);
    let starts = pick_starts(game.state(), rules, &nations, cfg.players);
    super::sea_resources::support_starts(&mut game, &starts, cfg.seed);
    for (p, &tile) in starts.iter().enumerate() {
        let player = PlayerId(p as u8);
        game.set_player_nation(player, nations[p]);
        start_player(&mut game, player, nations[p], tile);
    }
    game
}

/// Classic start of one player: the nation's free techs, then its
/// starting units on the start tile, founder first (the AI founds with its
/// first unit). Content without starting units: one settler (kind 0
/// fallback for empty test content).
pub fn start_player<M: RulesModule>(game: &mut WorldBuilder<'_, M>, player: PlayerId, nation: u8, tile: TileId) {
    let rules = game.rules();
    if let Some(def) = rules.nations.get(nation as usize) {
        for tech in &def.free_techs {
            game.grant_tech(player, *tech);
        }
    }
    let mut units = rules.starting_units(nation);
    if units.is_empty() {
        units.push(settler_kind(rules));
    }
    let founder = |kind: &UnitKindId| rules.units.get(kind.0 as usize).map(|u| u.founder);
    units.sort_by_key(|kind| founder(kind) != Some(true));
    for kind in units {
        game.spawn_unit(player, tile, kind);
    }
}

/// Nation assignment: P0 = the chosen human nation, the rest are unique
/// from the seed-shuffled pool (Fisher-Yates). Deterministic: the same
/// seed and choice => the same arrangement. Fewer nations than players: repeat
/// cyclically (fallback; there are 10 in content, players <= 8).
pub fn assign_nations<M: RulesModule>(
    rules: &CompiledRuleset<M>,
    players: u8,
    human_nation: Option<u8>,
    seed: u64,
) -> Vec<u8> {
    let want = players as usize;
    let cap = rules.nations.len().min(u8::MAX as usize) as u8;
    if want == 0 {
        return vec![];
    }
    if cap == 0 {
        return vec![0; want]; // fixtures without nations: the default, hurts nobody
    }
    let mut pool: Vec<u8> = (0..cap).collect();
    let mut rng = XorShift64(seed ^ 0xC1_5EED);
    for i in (1..pool.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        pool.swap(i, j);
    }
    let mut out = Vec::with_capacity(want);
    if let Some(h) = human_nation {
        out.push(h % cap);
    }
    for &c in &pool {
        if out.len() >= want {
            break;
        }
        if !out.contains(&c) {
            out.push(c);
        }
    }
    let mut i = 0;
    while out.len() < want {
        out.push(pool[i % pool.len()]);
        i += 1;
    }
    out
}

/// Minimum spread of starts (hexes). With too few candidates it is relaxed:
/// the greedy pick takes the maximum of what is available, deterministically.
pub const MIN_START_DIST: i32 = 12;
/// Margin of starts from the map edge (rows/columns).
pub const START_MARGIN: i32 = 3;

/// Start positions: a fair pick, only land with a margin from the edge,
/// greedy farthest-point (spread), a score of ground/ring/water quality and
/// the nation bias (coastal is closer to the sea). Only map data, no RNG
/// (the seed has already played its part in terrain generation).
pub fn pick_starts<M: RulesModule>(game: &GameState<M>, rules: &CompiledRuleset<M>, nations: &[u8], players: u8) -> Vec<TileId> {
    let map = game.view().auth.map();
    let dims = map.dimensions;
    let w = dims.width as i32;
    let h = dims.height as i32;
    let tiles = dims.tile_count();
    let palette = rules.terrain_kinds.palette();
    let ground = |t: TileId| map.terrain(t).and_then(|code| palette.ground(code.0));

    // 1. Distance to water: multi-source BFS from all water tiles.
    let mut water_dist = vec![i32::MAX; tiles];
    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, d) in water_dist.iter_mut().enumerate() {
        if ground(TileId(i as u32)) == Some(Ground::Water) {
            *d = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        let d = water_dist[i];
        let c = dims.to_coord(TileId(i as u32)).expect("in-bounds tile");
        for dir in HEX_DIRS {
            if let Some(n) = dims.to_tile(HexCoord {
                q: c.q + dir.q,
                r: c.r + dir.r,
            }) {
                let ni = n.0 as usize;
                if water_dist[ni] > d + 1 {
                    water_dist[ni] = d + 1;
                    queue.push_back(ni);
                }
            }
        }
    }

    let passable = |t: TileId| crate::navigation::cost::enter_cost_on_map(map, t).is_some();

    // 2. Candidate score: ground + ring of 6 neighbours + water access + bias.
    let forest = rules.roles.feature(FeatureRole::Forest);
    let jungle = rules.roles.feature(FeatureRole::Jungle);
    // Classic region priorities: +4 per tile of the start ring in a
    // preferred region, -4 per tile in an avoided one.
    let region_of = |t: TileId, region: StartRegion| -> bool {
        match region {
            StartRegion::Grass => ground(t) == Some(Ground::Flat(Climate::Humid)),
            StartRegion::Plains => ground(t) == Some(Ground::Flat(Climate::Temperate)),
            StartRegion::Desert => ground(t) == Some(Ground::Flat(Climate::Arid)),
            StartRegion::Hills => ground(t) == Some(Ground::Hills),
            StartRegion::Forest => forest.is_some() && map.feature(t) == Some(forest),
            StartRegion::Tundra => ground(t) == Some(Ground::Flat(Climate::Cold)),
            StartRegion::Jungle => jungle.is_some() && map.feature(t) == Some(jungle),
        }
    };
    let region_score = |i: usize, regions: &StartRegions| -> i32 {
        if regions.prefer.is_empty() && regions.avoid.is_empty() {
            return 0;
        }
        let coord = dims.to_coord(TileId(i as u32)).expect("in-bounds tile");
        let mut ring = vec![TileId(i as u32)];
        for dir in HEX_DIRS {
            if let Some(n) = dims.to_tile(HexCoord {
                q: coord.q + dir.q,
                r: coord.r + dir.r,
            }) {
                ring.push(n);
            }
        }
        let mut score = 0;
        for t in ring {
            score += 4 * regions.prefer.iter().filter(|r| region_of(t, **r)).count() as i32;
            score -= 4 * regions.avoid.iter().filter(|r| region_of(t, **r)).count() as i32;
        }
        score
    };
    let score_tile = |i: usize, bias: StartBias| -> i32 {
        let coord = dims.to_coord(TileId(i as u32)).expect("in-bounds tile");
        if map.terrain(TileId(i as u32)).is_none() {
            return i32::MIN / 4;
        }
        let terrain = ground(TileId(i as u32));
        let (food, prod) = start_food_prod(terrain);
        let base = match terrain {
            Some(Ground::Flat(Climate::Humid)) => 6,     // grass is top
            Some(Ground::Flat(Climate::Temperate)) => 5, // plains
            Some(Ground::Hills) => 4,                    // hills — prod
            Some(Ground::Flat(Climate::Arid)) => 2,      // desert is dry, but one can live there
            Some(Ground::Flat(Climate::Frozen)) => -6,   // snow is the worst
            Some(Ground::Flat(Climate::Cold)) => 0,
            _ => 1, // water does not get here: passable already filtered it out
        };
        let mut ring_food = food;
        let mut ring_prod = prod;
        for dir in HEX_DIRS {
            if let Some(n) = dims.to_tile(HexCoord {
                q: coord.q + dir.q,
                r: coord.r + dir.r,
            }) {
                if map.terrain(n).is_some() {
                    let (f, p) = start_food_prod(ground(n));
                    ring_food += f;
                    ring_prod += p;
                }
            }
        }
        let mut score = base * 2 + ring_food.min(9) + ring_prod.min(5);
        let wd = water_dist[i];
        if wd == i32::MAX {
            // No water on the map: there can be no "coastal" without a sea.
            if matches!(bias, StartBias::Coastal) {
                score -= 24;
            }
        } else {
            // General closeness to water: 1-4 tiles, fish/port nearby.
            score += (5 - wd).clamp(0, 4) * 2;
            if matches!(bias, StartBias::Coastal) {
                score += if wd <= 6 {
                    16 - (wd - 1) * 3
                } else {
                    -3 * (wd - 6)
                };
            }
        }
        score
    };

    // 3. Greedy: among tiles at least MIN_START_DIST from every chosen start
    // (when any is), the best land (Classic never starts a nation on
    // the polar fringe because it is far away), then the farthest, then the
    // lower index. Distance first pushed starts to the poles on Classic maps.
    let mut chosen: Vec<HexCoord> = Vec::new();
    let mut starts: Vec<TileId> = Vec::with_capacity(players as usize);
    for p in 0..players as usize {
        let nation_def = nations.get(p).and_then(|&c| rules.nations.get(c as usize));
        let regions = nation_def.map(|c| c.start_regions.clone()).unwrap_or_default();
        let bias = match nation_def {
            Some(_) if regions.along_ocean => StartBias::Coastal,
            Some(c) => c.start_bias,
            None => StartBias::default(),
        };
        let mut best: Option<(bool, i32, i32, usize, TileId)> = None;
        for i in 0..tiles {
            let coord = dims.to_coord(TileId(i as u32)).expect("in-bounds tile");
            if coord.q < START_MARGIN
                || coord.r < START_MARGIN
                || coord.q >= w - START_MARGIN
                || coord.r >= h - START_MARGIN
            {
                continue;
            }
            let t = TileId(i as u32);
            if !passable(t) {
                continue;
            }
            let md = chosen
                .iter()
                .map(|&o| coord.distance(o))
                .min()
                .unwrap_or(i32::MAX);
            let meets = md >= MIN_START_DIST;
            let score = score_tile(i, bias) + region_score(i, &regions);
            let better = match best {
                None => true,
                Some(b) => meets
                    .cmp(&b.0)
                    .then(score.cmp(&b.2))
                    .then(md.cmp(&b.1))
                    .then(b.3.cmp(&i))
                    .is_gt(),
            };
            if better {
                best = Some((meets, md, score, i, t));
            }
        }
        match best {
            Some((_, _, _, _, t)) => {
                chosen.push(dims.to_coord(t).expect("in-bounds tile"));
                starts.push(t);
            }
            None => {
                // The map is smaller than the margin/candidates: a fallback so that
                // a settler is guaranteed to appear (the old behaviour).
                let t = (0..tiles)
                    .map(|i| TileId(i as u32))
                    .find(|&t| passable(t) && !starts.contains(&t))
                    .or_else(|| (0..tiles).map(|i| TileId(i as u32)).find(|&t| passable(t)))
                    .unwrap_or(TileId(0));
                starts.push(t);
            }
        }
    }
    starts
}


/// Food and production the start placement scores a ground with (a
/// placement heuristic of the generator, not the game's tile yields:
/// `derived::tile_yields` has those).
fn start_food_prod(ground: Option<Ground>) -> (i32, i32) {
    match ground {
        Some(Ground::Flat(Climate::Temperate)) => (1, 1),
        Some(Ground::Flat(Climate::Humid)) => (2, 1),
        Some(Ground::Hills) => (1, 2),
        Some(Ground::Flat(Climate::Arid)) => (0, 1),
        Some(Ground::Water | Ground::Mountains) => (0, 0),
        Some(Ground::Flat(Climate::Frozen | Climate::Cold)) => (1, 0),
        None => (1, 1),
    }
}

/// Copies a new deposit of `resource` on `tile` holds: one of the
/// resource's quantities, picked by the
/// tile so that map generation draws nothing more from its random stream;
/// 1 for a resource without quantities.
pub fn deposit_amount<M: RulesModule>(rules: &CompiledRuleset<M>, resource: crate::ids::ResourceId, tile: TileId) -> u8 {
    let Some(def) = rules.resources.get(resource.0 as usize) else {
        return 1;
    };
    if def.quantities.is_empty() {
        return 1;
    }
    let mixed = (u64::from(tile.0).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 32) as usize;
    def.quantities[mixed % def.quantities.len()].max(1)
}
