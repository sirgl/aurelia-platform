//! The surface field: a fixed order of stages over contributors (platform/MESHING.md §2.2).
//!
//! | Stage | Operation | Contributors |
//! |---|---|---|
//! | 1 base | weighted cell mix | biomes (`biomes.rs`) |
//! | 2 forms | `Add` | hills (`biomes.rs`), mountain massifs (`massifs.rs`) |
//! | 3 hydrology | `Transform` | sea floor and beach, river bed and banks (`banks.rs`) |
//! | 4 cutouts | `Transform`, partition of unity | rocky coast (`cliffs.rs`, `cutouts.rs`) |
//! | 5 protection | `Clamp` | the mesher: wet contacts at the water level (`surface.rs`) |
//!
//! Contributors read only the world observation (known cells, edges, rivers),
//! never the mesh. Materials (sand, stone) pass through the same stages and
//! become a color at the end. Massif lift is computed in stage 2 but rides on
//! top of the shaped level, see `massifs.rs`.
use super::{
    banks::{self, Shape, COAST_EXTENT, SEA_LEVEL},
    biomes::{smooth, Biomes, Ground},
    cliffs::CoastGeology,
    cutouts,
    fingerprint::{Sink, Tag},
    hydrology::{Hydrology, Point, River},
    index::SegmentIndex,
    massifs::Massifs,
    pads::{self, Pad, PadKind, Pads},
    sites::Sites,
    transitions::{CoastProfile, CoastSection, CoastShape},
    walls::{CliffWalls, CoastNormals, ROWS},
    MountainVertex, SurfaceVertex, SurfaceWaterKind,
};
use crate::{
    scene::{
        fog::{fog_at, fog_at_position},
        layout::{tile_to_world, EDGE_NEIGHBORS},
        picking::world_to_tile,
        style::CompiledTerrainStyle,
        terrain::{corner_lattice, lattice_to_world},
    },
    world::state::{RenderWorld, TerrainCell},
};
use kernel::ids::TileId;

pub struct Landscape<'a> {
    world: &'a RenderWorld,
    style: &'a CompiledTerrainStyle,
    biomes: Biomes,
    sites: Sites,
    pads: Pads,
    hydrology: Hydrology,
    geology: CoastGeology,
    walls: CliffWalls,
    massifs: Massifs,
    mountains: Centers,
    hills: Centers,
    /// Every known cell: what lies farther than `KNOWN_BAND` from all of them is parchment.
    known_cells: Centers,
}
/// Requested edge lengths of the sizing field (platform/MESHING.md §2.4), world units.
/// Each request covers a band of a distance; the field is their minimum.
const PLAIN_EDGE: f32 = 0.45;
const COAST_EDGE: f32 = 0.15;
const COAST_BAND: f32 = 0.7;
const RIVER_EDGE: f32 = 0.2;
const RIVER_BAND: f32 = 0.6;
const RELIEF_EDGE: f32 = 0.2;
/// Mountain massifs reach about 2.8 from their cell; hill shoulders about 1.2.
const MOUNTAIN_BAND: f32 = 2.9;
const HILL_BAND: f32 = 1.25;
/// Rocky coast: the wall band up to just past the brow, and a little water.
const CLIFF_EDGE: f32 = 0.1;
const CLIFF_BAND: [f32; 2] = [-0.25, 1.2];
/// A coast segment is drawn as a wall where the cliff share passes this.
const ROCKY: f32 = 0.5;
/// Nobody sees the ground under the parchment: it gets the coarsest lattice
/// level beyond this distance from every known cell centre (a known cell's
/// own area reaches 1.0 from its centre; the rest is the fog transition).
const UNKNOWN_EDGE: f32 = super::sizing::MAX_EDGE;
const KNOWN_BAND: f32 = 1.8;
/// Pads keep off a rocky shore within this distance of the waterline (the wall band ends at 0.8).
const CLIFF_OWNS: f32 = 0.9;

/// Centres of known cells of one terrain, for distance queries.
struct Centers {
    points: Vec<Point>,
    index: SegmentIndex,
}
impl Centers {
    fn new(world: &RenderWorld, terrain: u16) -> Self {
        Self::of(world, |c| c.terrain == terrain)
    }
    /// Centres of the known cells that `keep` accepts.
    fn of(world: &RenderWorld, keep: impl Fn(&TerrainCell) -> bool) -> Self {
        let points: Vec<Point> = world
            .terrain
            .iter()
            .filter(|c| keep(c) && fog_at(world, c.tile) >= 0.4)
            .map(|c| {
                let (x, z) = tile_to_world(c.tile.0 % world.width, c.tile.0 / world.width);
                [x, z]
            })
            .collect();
        let index = SegmentIndex::new(points.iter().map(|&p| [p, p]), 2.0);
        Self { points, index }
    }
    fn within(&self, p: Point, reach: f32) -> bool {
        let distance = |i: u32| (p[0] - self.points[i as usize][0]).hypot(p[1] - self.points[i as usize][1]);
        self.index.any_within(p, reach, distance)
    }
}
#[derive(Clone, Copy)]
pub struct WaterSample {
    pub kind: SurfaceWaterKind,
    pub level: f32,
    pub flow: [f32; 2],
}

/// What a point sees of the known world.
#[derive(Clone, Copy)]
struct Probe {
    known: bool,
    /// Signed distance to the known coast, negative at sea.
    shore: f32,
}

/// The field at one point: the shaped ground vertex and the massif relief
/// that stage 5 adds unless the vertex is a wet contact.
#[derive(Debug, Clone, Copy)]
pub struct FieldSample {
    pub vertex: SurfaceVertex,
    pub relief: MountainVertex,
}

impl FieldSample {
    /// The vertex with the massif relief added and painted.
    pub fn lifted(&self) -> SurfaceVertex {
        let mut v = self.vertex;
        if self.relief.lift > 0. {
            v.position[1] += self.relief.lift;
            v.albedo = super::massifs::paint(v.albedo, [v.position[0], v.position[2]], self.relief.lift);
        }
        v
    }
}

impl<'a> Landscape<'a> {
    /// The contributors read independent parts of the observation, so they
    /// are derived side by side; the cliff walls need the coast and geology.
    pub fn new(world: &'a RenderWorld, style: &'a CompiledTerrainStyle) -> Self {
        let biomes = Biomes::new(world);
        #[cfg(not(target_arch = "wasm32"))]
        let (hydrology, geology, massifs, [mountains, hills, known_cells]) = std::thread::scope(|scope| {
            let hydrology = scope.spawn(|| Hydrology::new(world, |x, z| biomes.ground(style, x, z).height));
            let geology = scope.spawn(|| CoastGeology::new(world));
            let massifs = scope.spawn(|| Massifs::new(world));
            let centers = [Centers::new(world, 6), Centers::new(world, 2), Centers::of(world, |_| true)];
            let join = "surface contributor";
            (hydrology.join().expect(join), geology.join().expect(join), massifs.join().expect(join), centers)
        });
        // A browser page has no threads to scope: the contributors are derived in turn.
        #[cfg(target_arch = "wasm32")]
        let (hydrology, geology, massifs, [mountains, hills, known_cells]) = (
            Hydrology::new(world, |x, z| biomes.ground(style, x, z).height),
            CoastGeology::new(world),
            Massifs::new(world),
            [Centers::new(world, 6), Centers::new(world, 2), Centers::of(world, |_| true)],
        );
        let sites = Sites::new(world, &biomes, style);
        let mut field = Self {
            world,
            style,
            biomes,
            sites,
            pads: Pads::default(),
            hydrology,
            geology,
            walls: CliffWalls::default(),
            massifs,
            mountains,
            hills,
            known_cells,
        };
        field.walls = field.cliff_walls();
        if crate::debug_view::slope_mode_new() {
            field.pads = field.make_pads();
        }
        field
    }

    /// Pads under the known city tiles and the known tiles with an improvement or a resource,
    /// where the natural ground (this field, without pads) is uneven enough and not a cliff.
    fn make_pads(&self) -> Pads {
        let world = self.world;
        let mut wanted: Vec<(u32, PadKind)> = world.cities.iter().map(|c| (c.tile.0, PadKind::City)).collect();
        let city_tiles: std::collections::HashSet<u32> = wanted.iter().map(|w| w.0).collect();
        for (i, cell) in world.terrain.iter().enumerate() {
            let tile = i as u32;
            let prop = world.improvements.get(i).copied().flatten().is_some() || cell.resource.is_some_and(|r| world.unlocked_resources.contains(&r));
            let barren = world.terrain_kinds.is_water(cell.terrain) || world.terrain_kinds.is_mountains(cell.terrain) || world.natural_wonders.get(i).copied().flatten().is_some();
            if prop && !barren && !city_tiles.contains(&tile) {
                wanted.push((tile, PadKind::Tile));
            }
        }
        let mut list = Vec::new();
        for (tile, kind) in wanted {
            if fog_at(world, TileId(tile)) < 0.4 {
                continue;
            }
            let (x, z) = tile_to_world(tile % world.width, tile / world.width);
            let samples: Vec<(f32, bool)> = kind
                .disc([x, z])
                .iter()
                .map(|&p| {
                    let sample = self.sample(p[0], p[1], None);
                    let dry = self.probe(p[0], p[1]).shore >= 0.3 && sample.vertex.water_depth <= 0.;
                    (sample.vertex.position[1] + sample.relief.lift, dry)
                })
                .collect();
            if let Some((level, tan)) = pads::level_of(kind, &samples) {
                list.push(Pad { centre: [x, z], level, outer: kind.outer_for(tan), kind, tile });
            }
        }
        Pads::new(list)
    }

    /// How much of a pad's cut the point at `p` accepts: none where the coast, a river or a cliff
    /// owns the ground (their contours and water contacts stay exact), a smooth fade around.
    fn pad_allowed(&self, p: Point, shore: f32, river: Option<(&River, f32, f32)>) -> f32 {
        let coast = smooth((shore - 0.25) / 0.35);
        if coast <= 0. {
            return 0.;
        }
        let corridor = river.map_or(1., banks::outside_corridor);
        if corridor <= 0. {
            return 0.;
        }
        // The cliff wall and its brow own the ground close to a rocky shore; farther inland the
        // plateau is plain ground a pad may level.
        let rock = self.cliff_share(p, river) * (1. - smooth((shore - CLIFF_OWNS) / 0.5));
        coast * corridor * (1. - smooth(rock / 0.5))
    }
    pub fn world(&self) -> &'a RenderWorld {
        self.world
    }
    /// Feeds every contributor's items to the regions they can reach (`fingerprint.rs`).
    pub fn fingerprint(&self, sink: &mut Sink) {
        self.cells_fingerprint(sink);
        self.hydrology.fingerprint(sink);
        self.geology.fingerprint(sink);
        self.walls.fingerprint(sink);
        self.massifs.fingerprint(sink);
        self.sites.fingerprint(sink);
        self.pads.fingerprint(sink);
        if let Some(relief) = crate::debug_view::relief() {
            let (w, h) = (self.world.width, self.world.height);
            let (x1, z1) = tile_to_world(w.saturating_sub(1), h.saturating_sub(1));
            sink.item(Tag::Relief, [-2., -2.], [x1 + 2., z1 + 2.], &relief.words());
        }
    }
    /// Per cell: its look (which climate it wears), knowledge, wonder and river flag.
    fn cells_fingerprint(&self, sink: &mut Sink) {
        let world = self.world;
        for cell in &world.terrain {
            let i = cell.tile.0 as usize;
            let (x, z) = tile_to_world(cell.tile.0 % world.width, cell.tile.0 / world.width);
            let look = self.biomes.look_key(i);
            let wonder = world.natural_wonders.get(i).copied().flatten().map_or(u32::MAX, u32::from);
            let river = world.river.get(i).copied().unwrap_or(0);
            let words = [look as u32, (look >> 32) as u32, fog_at(world, cell.tile).to_bits(), wonder, u32::from(river), u32::from(cell.terrain)];
            sink.item(Tag::Cell, [x - 1., z - 1.], [x + 1., z + 1.], &words);
        }
    }
    fn cliff_walls(&self) -> CliffWalls {
        let coasts = &self.hydrology.coasts;
        let rocky = |i: usize| {
            let c = &coasts[i];
            self.cliff_share_at([(c.a[0] + c.b[0]) * 0.5, (c.a[1] + c.b[1]) * 0.5]) >= ROCKY
        };
        CliffWalls::new(coasts, rocky, |p, inland, depth| self.room(p, inland, depth))
    }
    /// How far inland from coast point `p` this shore stays the nearest one:
    /// past the middle of a narrow peninsula the opposite shore is closer.
    fn room(&self, p: Point, inland: Point, depth: f32) -> f32 {
        const STEP: f32 = 0.04;
        let mut d = STEP;
        while d <= depth + STEP {
            let shore = self.sea_distance(p[0] + inland[0] * d, p[1] + inland[1] * d);
            if shore < 0.8 * d - 0.02 {
                return d - STEP;
            }
            d += STEP;
        }
        f32::INFINITY
    }
    /// Smallest requested edge length within `slack` of `p`. Every band is a
    /// band of a distance that changes at most as fast as `p` moves.
    pub fn edge_length(&self, p: Point, slack: f32) -> f32 {
        if !self.known_cells.within(p, KNOWN_BAND + slack) {
            return UNKNOWN_EDGE;
        }
        let mut edge = PLAIN_EDGE;
        let shore = self.sea_distance(p[0], p[1]);
        if shore.abs() < COAST_BAND + slack {
            edge = edge.min(COAST_EDGE);
        }
        let river = self.hydrology.river(p);
        if river.is_some_and(|(r, d, t)| d - r.width_at(t) < RIVER_BAND + slack) {
            edge = edge.min(RIVER_EDGE);
        }
        if let Some(site) = self.sites.edge_length(p, slack) {
            edge = edge.min(site);
        }
        if let Some(pad) = self.pads.edge_length(p, slack) {
            edge = edge.min(pad);
        }
        if let Some(fine) = crate::debug_view::relief().and_then(|r| r.edge_length(p[0], p[1])) {
            edge = edge.min(fine);
        }
        if edge > RELIEF_EDGE && (self.mountains.within(p, MOUNTAIN_BAND + slack) || self.hills.within(p, HILL_BAND + slack)) {
            edge = RELIEF_EDGE;
        }
        let in_band = shore > CLIFF_BAND[0] - slack && shore < CLIFF_BAND[1] + slack;
        if in_band && self.cliff_share(p, river) > 0.2 {
            edge = edge.min(CLIFF_EDGE);
        }
        edge
    }
    /// The profile row of a cliff wall vertex at exactly this position.
    pub fn wall_row(&self, p: Point) -> Option<f32> {
        self.walls.fraction_at(p)
    }
    /// Between the foot and the brow of a cliff only wall contours place
    /// vertices: inside a wall strip, or short of the brow where a narrow
    /// peninsula compressed the wall, so its top stays level.
    pub fn inside_wall(&self, x: f32, z: f32) -> bool {
        if self.walls.contains([x, z]) {
            return true;
        }
        let shore = self.sea_distance(x, z);
        // The widest profile is 0.80.
        if shore <= 0. || shore >= ROWS[ROWS.len() - 1] * 0.80 {
            return false;
        }
        let brow = ROWS[ROWS.len() - 1] * CoastShape::at([x, z]).width;
        shore < brow && self.cliff_share_at([x, z]) >= ROCKY
    }
    fn known(&self, x: f32, z: f32) -> bool {
        self.probe(x, z).known
    }
    fn sea_distance(&self, x: f32, z: f32) -> f32 {
        self.probe(x, z).shore
    }
    /// One tile lookup and one coast query per point; unknown cells see no water.
    fn probe(&self, x: f32, z: f32) -> Probe {
        let tile = world_to_tile(x, z, (self.world.width, self.world.height));
        let Some(id) = tile.filter(|&id| fog_at(self.world, id) >= 0.4) else {
            return Probe { known: false, shore: 3. };
        };
        let shore = match self.hydrology.coast([x, z]) {
            Some((distance, _)) => distance,
            None if self.world.is_water(id.0 as usize) => -3.,
            None => 3.,
        };
        Probe { known: true, shore }
    }
    pub fn water_at(&self, x: f32, z: f32) -> WaterSample {
        let probe = self.probe(x, z);
        // Seaward of the coast the channel does not matter.
        let channel = if probe.known && probe.shore >= -0.00001 { self.hydrology.channel([x, z]) } else { None };
        Self::water(probe, channel, [x, z])
    }
    /// Water at a point: the sea seaward of the known coast, a river inside its channel.
    fn water(probe: Probe, channel: Option<(&River, f32, f32)>, p: Point) -> WaterSample {
        let none = WaterSample { kind: SurfaceWaterKind::None, level: SEA_LEVEL, flow: [0.; 2] };
        if !probe.known {
            return none;
        }
        if probe.shore < -0.00001 {
            return WaterSample { kind: SurfaceWaterKind::Sea, ..none };
        }
        match channel {
            Some((r, _, t)) => WaterSample {
                kind: SurfaceWaterKind::River,
                level: r.ya * (1. - t) + r.yb * t,
                flow: r.flow_at(p, t),
            },
            _ => none,
        }
    }
    pub fn water_for_kind(&self, x: f32, z: f32, kind: SurfaceWaterKind) -> WaterSample {
        if kind == SurfaceWaterKind::River {
            if let Some((r, _, t)) = self.hydrology.river([x, z]) {
                return WaterSample {
                    kind,
                    level: r.ya * (1. - t) + r.yb * t,
                    flow: r.flow_at([x, z], t),
                };
            }
        }
        WaterSample { kind, level: SEA_LEVEL, flow: [x, z] }
    }
    /// The shaped ground vertex at a point, stages 1–4 without massif relief.
    #[cfg(test)]
    pub fn vertex(&self, x: f32, z: f32) -> SurfaceVertex {
        self.sample(x, z, None).vertex
    }
    /// The field at a point. A wall vertex takes the profile height of its
    /// row, wherever a fracture moved it; `seam_row` is the row of a wall
    /// contour point created where a region seam split the contour.
    pub fn sample(&self, x: f32, z: f32, seam_row: Option<f32>) -> FieldSample {
        let p = [x, z];
        let mut probe = self.probe(x, z);
        if let Some(fraction) = self.walls.fraction_at(p).or(seam_row).filter(|_| probe.known) {
            probe.shore = fraction * CoastShape::at(p).width;
        }
        let hit = if probe.known { self.hydrology.nearest_river(p) } else { None };
        let river = self.hydrology.river_at(hit);
        // Stages 1 and 2.
        let ground = self.biomes.ground(self.style, x, z);
        let fog = fog_at_position(self.world, x, z);
        let mut relief = self.massifs.lift(self.world, p, fog, river);
        // Stages 3 and 4; a natural wonder site owns its ground, massif relief gives way.
        let mut shape = self.shape(p, probe.shore, &ground, river);
        // Developer probe (gallery): a synthetic relief over the generated ground; never set in the game.
        if let Some(relief) = crate::debug_view::relief().filter(|_| probe.known && probe.shore >= 0.) {
            shape.y += relief.height(x, z);
        }
        let test_relief = crate::debug_view::relief();
        let shift = |c: Point| test_relief.map_or(0., |r| r.height(c[0], c[1]));
        let (y, owned) = self.sites.cut_shifted(p, shape.y, probe.known && probe.shore < 0., &shift);
        shape.y = y;
        relief.lift *= 1. - owned;
        relief.snow *= 1. - owned;
        // A pad under an object tile: cut and fill the total height (ground and massif lift).
        if probe.known && probe.shore >= 0. && self.pads.reaches(p, 0.) {
            let allowed = self.pad_allowed(p, probe.shore, river) * (1. - owned);
            let (total, share) = self.pads.cut(p, shape.y + relief.lift, allowed);
            if share > 0. {
                relief.lift *= 1. - share;
                relief.snow *= 1. - share;
                shape.y = total - relief.lift;
            }
        }
        let channel = self.hydrology.channel_at(p, hit);
        let water = Self::water(probe, channel, p);
        let vertex = SurfaceVertex {
            position: [x, shape.y, z],
            albedo: shape.albedo,
            terrain: ground.terrain,
            fallout: if probe.known && water.kind == SurfaceWaterKind::None { ground.fallout } else { 0. },
            coastal_rock: if probe.known && water.kind == SurfaceWaterKind::None { shape.stone } else { 0. },
            water: 2. * (1. - smooth(shape.shore.max(0.) / 0.30)),
            water_depth: if water.kind == SurfaceWaterKind::None { 0. } else { (water.level - shape.y).max(0.) },
            shore_distance: shape.shore,
            fog,
            water_kind: SurfaceWaterKind::None,
            flow_uv: water.flow,
            tone: river.map_or([0.; 3], |(r, _, t)| [r.mouth_at(t), banks::water_tint(ground.tint, r, t), banks::rapids_of(r)]),
        };
        FieldSample { vertex, relief }
    }
    /// Stage 3, hydrology: the sea floor offshore, the beach in the coast
    /// band, then the river bed and banks. Stage 4: the rocky-coast cutout,
    /// which gives way to a river corridor. Colors come from the materials last.
    fn shape(&self, p: Point, shore: f32, ground: &Ground, river: Option<(&River, f32, f32)>) -> Shape {
        let in_coast = (0. ..COAST_EXTENT).contains(&shore);
        let profile = if in_coast { CoastShape::at(p) } else { CoastShape::default() };
        let section = if shore < 0. {
            CoastSection { height: banks::sea_floor(shore), sand: 0., stone: 0. }
        } else if in_coast {
            banks::beach(shore, ground, profile)
        } else {
            CoastSection { height: ground.height, sand: 0., stone: 0. }
        };
        let mut shape = Shape { y: section.height, albedo: ground.color, stone: section.stone, shore };
        // The sea floor is not a river bed: a river's carving stops at the coast, where its
        // water is the sea's and its bed shoals to the floor (`River::depth_at`).
        if river.is_some() && shore >= 0. {
            // Every river that reaches the point carves from the same ground; the deepest
            // wins, so the ground is continuous where the nearest river changes.
            let ground_y = shape.y;
            let mut carved = Shape { y: ground_y, albedo: shape.albedo, stone: shape.stone, shore: shape.shore };
            for around in self.hydrology.rivers_around(p, banks::MAX_REACH) {
                let mut one = Shape { y: ground_y, albedo: shape.albedo, stone: shape.stone, shore: shape.shore };
                one.carve_river(around);
                carved.y = carved.y.min(one.y);
                carved.stone = carved.stone.min(one.stone);
                carved.shore = carved.shore.min(one.shore);
            }
            shape = carved;
        }
        if in_coast {
            let carved = CoastSection { height: shape.y, sand: section.sand, stone: shape.stone };
            let cut = CoastGeology::cut(p, shore, ground, profile, self.cliff_share(p, river));
            let cut = cutouts::partition(carved, &[cut]);
            shape.y = cut.height;
            shape.stone = cut.stone;
            shape.albedo = banks::coast_albedo(ground.color, cut);
        }
        if let Some(river) = river {
            let rise = (section.height - banks::river_level(river)).max(0.);
            let look = banks::BankLook {
                tint: banks::water_tint(ground.tint, river.0, river.2),
                size: river.0.size_at(river.2),
                spring: river.0.spring_at(river.2),
            };
            shape.albedo = banks::river_bank_color(shape.albedo, p, river.1 - river.0.width_at(river.2), rise, shore, look);
        }
        shape
    }
    /// The cliff share of the coast at a point: the geology, giving way to a
    /// river corridor, since a cutout must never dam water.
    fn cliff_share(&self, p: Point, river: Option<(&River, f32, f32)>) -> f32 {
        let rock = self.geology.rock(p);
        match river {
            Some(river) if rock > 0. => rock * banks::outside_corridor(river),
            _ => rock,
        }
    }
    fn cliff_share_at(&self, p: Point) -> f32 {
        self.cliff_share(p, self.hydrology.river(p))
    }
    /// Lake cells of the known world (`Biomes::lakes`).
    pub fn lakes(&self) -> Vec<bool> {
        self.biomes.lakes()
    }
    pub fn wonder_sites(&self) -> &[super::sites::WonderSite] {
        &self.sites.list
    }
    pub fn coast_props(&self) -> &[super::CoastProp] {
        &self.walls.props
    }
    pub fn river_segments(&self) -> &[super::hydrology::River] {
        &self.hydrology.rivers
    }
    /// Contours: river banks, plain coast profile lines and cliff walls.
    pub fn constraints(&self) -> Vec<[Point; 2]> {
        let mut lines = self.hydrology.river_constraints();
        let normals = CoastNormals::new(&self.hydrology.coasts);
        for (i, c) in self.hydrology.coasts.iter().enumerate() {
            if self.walls.replaced[i] {
                continue;
            }
            lines.push([c.a, c.b]);
            self.profile_lines(c, &normals, &mut lines);
        }
        lines.extend_from_slice(&self.walls.contours);
        lines.extend(self.fog_water_edges());
        lines
    }
    /// Hex edges between a known sea cell and an unknown one: the known water
    /// ends exactly there instead of on the jagged centroids of wide faces.
    fn fog_water_edges(&self) -> Vec<[Point; 2]> {
        let world = self.world;
        let known = |tile: TileId| fog_at(world, tile) >= 0.4;
        let mut edges = vec![];
        for cell in &world.terrain {
            if !world.terrain_kinds.is_water(cell.terrain) || !known(cell.tile) {
                continue;
            }
            let (q, r) = ((cell.tile.0 % world.width) as i32, (cell.tile.0 / world.width) as i32);
            for (d, &(dq, dr)) in EDGE_NEIGHBORS.iter().enumerate() {
                let (nq, nr) = (q + dq, r + dr);
                let inside = nq >= 0 && nr >= 0 && nq < world.width as i32 && nr < world.height as i32;
                if !inside || known(TileId(nr as u32 * world.width + nq as u32)) {
                    continue;
                }
                let (ax, az) = lattice_to_world(corner_lattice(q, r, d));
                let (bx, bz) = lattice_to_world(corner_lattice(q, r, (d + 1) % 6));
                edges.push([[ax, az], [bx, bz]]);
            }
        }
        edges
    }
    /// The profile contours of a plain coast segment, so its section is not
    /// flattened by a wide triangle.
    fn profile_lines(&self, c: &super::hydrology::Coast, normals: &CoastNormals, lines: &mut Vec<[Point; 2]>) {
        let (na, nb) = (normals.at(c.a), normals.at(c.b));
        for distance in CoastProfile::CONTOURS {
            let da = CoastShape::contour_offset(c.a, na, distance);
            let db = CoastShape::contour_offset(c.b, nb, distance);
            let a = [c.a[0] - na[0] * da, c.a[1] - na[1] * da];
            let b = [c.b[0] - nb[0] * db, c.b[1] - nb[1] * db];
            // Across a narrow peninsula an offset line would run into the
            // opposite shore; keep it only where it is a true level line.
            let level = |p: Point, d: f32| (self.sea_distance(p[0], p[1]) - d).abs() < 0.03;
            if self.known(a[0], a[1]) && self.known(b[0], b[1]) && level(a, da) && level(b, db) {
                lines.push([a, b]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nearest_opposite_shore_switch_has_no_height_or_material_seam() {
        use super::super::hydrology::Coast;
        use crate::{scene::style::{compile, default_style}, world::state::TerrainCell};
        let count = 30 * 8;
        let world = RenderWorld {
            width: 30, height: 8, visible: vec![1; count], explored: vec![1; count],
            terrain: (0..count).map(|i| TerrainCell {
                tile: TileId(i as u32), terrain: 1, owner: None, resource: None,
            }).collect(), ..Default::default()
        };
        let style = compile(default_style()).unwrap();
        // A narrow strip whose south shore is a cliff and north shore a beach.
        let (x0, z0) = (24., 4.);
        let hydrology = Hydrology::from_parts(vec![
            Coast { a: [x0 - 8., z0], b: [x0 + 8., z0], sea: [0., -1.] },
            Coast { a: [x0 - 8., z0 + 2.], b: [x0 + 8., z0 + 2.], sea: [0., 1.] },
        ], vec![]);
        let geology = CoastGeology::from_edges(
            vec![[[x0 - 8., z0], [x0 + 8., z0]]],
            vec![[[x0 - 8., z0 + 2.], [x0 + 8., z0 + 2.]]],
        );
        assert_eq!(geology.rock([x0, z0 + 0.1]), 1.);
        assert_eq!(geology.rock([x0, z0 + 1.9]), 0.);
        let field = Landscape {
            world: &world,
            style: &style,
            biomes: Biomes::new(&world),
            sites: Sites::default(),
            pads: Pads::default(),
            hydrology,
            geology,
            walls: CliffWalls::default(),
            massifs: Massifs::default(),
            mountains: Centers::new(&world, 6),
            hills: Centers::new(&world, 2),
            known_cells: Centers::of(&world, |_| true),
        };
        for i in 0..130 {
            let x = x0 - 6.5 + i as f32 * 0.1;
            let a = field.vertex(x, z0 + 1. - 0.0001);
            let b = field.vertex(x, z0 + 1. + 0.0001);
            assert!((a.position[1] - b.position[1]).abs() < 0.003, "height seam at {x}");
            assert!((a.coastal_rock - b.coastal_rock).abs() < 0.003, "material seam at {x}");
            for k in 0..3 { assert!((a.albedo[k] - b.albedo[k]).abs() < 0.003); }
        }
    }
}
