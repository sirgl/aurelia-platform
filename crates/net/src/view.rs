//! What the server tells a seat about the world: a whitelist, built from the player's own
//! knowledge (`PlayerView`, the one filter between the truth and the AI, the render projection
//! and the HUD) and the AI's observation of units (`kernel::ai::observation`). Nothing is
//! copied from the world by default: a field is here because a player may know it.
//!
//! A [`SeatView`] is sent whole when a seat joins or rejoins; after that the server sends
//! [`SeatDelta`]s, each the difference of two views of the same seat. A delta is a function of
//! two whitelisted views, so it cannot carry what a view does not.

use std::collections::BTreeMap;

use kernel::ids::{PlayerId, ProductionItem, TileId};
use kernel::knowledge::view::{KnownTile, TileSnapshot};
use kernel::module::RulesModule;
use kernel::rules::CompiledRuleset;
use kernel::world::map::Route;
use kernel::world::state::GameState;

use crate::wire::{Reader, Wire, WireError, Writer};

const NONE16: u16 = u16::MAX;
const NONE8: u8 = u8::MAX;

fn opt16(v: Option<u16>) -> u16 {
    v.unwrap_or(NONE16)
}

/// A tile's layers as a player knows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileSnap {
    pub terrain: u16,
    pub feature: u16,
    pub resource: u16,
    pub improvement: u16,
    /// 0 none, 1 road, 2 railroad.
    pub route: u8,
    pub river: bool,
    pub wonder: u16,
    pub owner: u8,
    pub city: bool,
    pub pillaged: bool,
}

impl TileSnap {
    fn of(s: &TileSnapshot, pillaged: bool) -> Self {
        Self {
            terrain: s.terrain.0,
            feature: opt16(s.feature.map(|f| f.0)),
            resource: opt16(s.resource.map(|r| r.0)),
            improvement: opt16(s.improvement.map(|i| i.0)),
            route: match s.route {
                Route::None => 0,
                Route::Road => 1,
                Route::Railroad => 2,
            },
            river: s.river,
            wonder: opt16(s.natural_wonder.map(|w| w.0)),
            owner: s.owner.map_or(NONE8, |p| p.0),
            city: s.city,
            pillaged,
        }
    }
}

impl Wire for TileSnap {
    fn put(&self, w: &mut Writer) {
        w.u16(self.terrain);
        w.u16(self.feature);
        w.u16(self.resource);
        w.u16(self.improvement);
        w.u8(self.route);
        w.u8(self.owner);
        w.u16(self.wonder);
        w.u8(self.river as u8 | (self.city as u8) << 1 | (self.pillaged as u8) << 2);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (terrain, feature, resource, improvement) = (r.u16()?, r.u16()?, r.u16()?, r.u16()?);
        let (route, owner, wonder, flags) = (r.u8()?, r.u8()?, r.u16()?, r.u8()?);
        if route > 2 || flags > 7 {
            return Err(WireError::Invalid("tile"));
        }
        Ok(Self {
            terrain,
            feature,
            resource,
            improvement,
            route,
            river: flags & 1 != 0,
            wonder,
            owner,
            city: flags & 2 != 0,
            pillaged: flags & 4 != 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileView {
    Unknown,
    Remembered { snap: TileSnap, seen_round: u32 },
    Visible(TileSnap),
}

impl Wire for TileView {
    fn put(&self, w: &mut Writer) {
        match self {
            Self::Unknown => w.u8(0),
            Self::Remembered { snap, seen_round } => {
                w.u8(1);
                snap.put(w);
                w.u32(*seen_round);
            }
            Self::Visible(snap) => {
                w.u8(2);
                snap.put(w);
            }
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::Remembered {
                snap: TileSnap::take(r)?,
                seen_round: r.u32()?,
            }),
            2 => Ok(Self::Visible(TileSnap::take(r)?)),
            _ => Err(WireError::Invalid("tile tag")),
        }
    }
}

/// What only the owner of a unit knows of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnUnit {
    pub moves_left: u16,
    pub moves_max: u16,
    pub fortified: bool,
    pub can_fortify: bool,
    pub can_attack: bool,
    pub experience: u32,
    pub level: u8,
    pub attacks_made: u8,
    /// The tile of a standing march order.
    pub goal: Option<u32>,
    pub working: bool,
}

impl Wire for OwnUnit {
    fn put(&self, w: &mut Writer) {
        w.u16(self.moves_left);
        w.u16(self.moves_max);
        w.u8(self.fortified as u8
            | (self.can_fortify as u8) << 1
            | (self.can_attack as u8) << 2
            | (self.working as u8) << 3);
        w.u32(self.experience);
        w.u8(self.level);
        w.u8(self.attacks_made);
        w.opt(&self.goal);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (moves_left, moves_max, flags) = (r.u16()?, r.u16()?, r.u8()?);
        if flags > 15 {
            return Err(WireError::Invalid("unit flags"));
        }
        Ok(Self {
            moves_left,
            moves_max,
            fortified: flags & 1 != 0,
            can_fortify: flags & 2 != 0,
            can_attack: flags & 4 != 0,
            working: flags & 8 != 0,
            experience: r.u32()?,
            level: r.u8()?,
            attacks_made: r.u8()?,
            goal: r.opt()?,
        })
    }
}

/// A unit the seat sees: its own, or another player's on a tile the seat sees now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitView {
    pub id: u64,
    pub tile: u32,
    pub owner: u8,
    pub kind: u16,
    pub hp: u16,
    pub own: Option<OwnUnit>,
}

impl Wire for UnitView {
    fn put(&self, w: &mut Writer) {
        w.u64(self.id);
        w.u32(self.tile);
        w.u8(self.owner);
        w.u16(self.kind);
        w.u16(self.hp);
        w.opt(&self.own);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            id: r.u64()?,
            tile: r.u32()?,
            owner: r.u8()?,
            kind: r.u16()?,
            hp: r.u16()?,
            own: r.opt()?,
        })
    }
}

/// What only the owner of a city knows of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnCity {
    pub production: u32,
    pub food_times100: u32,
    /// (0 unit, 1 building, 2 project, the content row).
    pub queue: Vec<(u8, u16)>,
}

impl Wire for OwnCity {
    fn put(&self, w: &mut Writer) {
        w.u32(self.production);
        w.u32(self.food_times100);
        w.u32(self.queue.len() as u32);
        for (tag, row) in &self.queue {
            w.u8(*tag);
            w.u16(*row);
        }
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let (production, food_times100) = (r.u32()?, r.u32()?);
        let n = r.u32()? as usize;
        if n > r.remaining() {
            return Err(WireError::TooLong);
        }
        let queue = (0..n)
            .map(|_| Ok((r.u8()?, r.u16()?)))
            .collect::<Result<_, WireError>>()?;
        Ok(Self {
            production,
            food_times100,
            queue,
        })
    }
}

/// A city the seat knows: its own, one it sees now, or one it remembers. A city is keyed by
/// its tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityView {
    pub tile: u32,
    pub owner: u8,
    pub name: String,
    /// None: seen once, the numbers unknown.
    pub population: Option<u32>,
    pub buildings: Vec<u16>,
    /// None: remembered, not seen now.
    pub seen_now: Option<SeenCity>,
    pub own: Option<OwnCity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeenCity {
    pub id: u64,
    pub hp: u16,
}

impl Wire for CityView {
    fn put(&self, w: &mut Writer) {
        w.u32(self.tile);
        w.u8(self.owner);
        w.str(&self.name);
        w.opt(&self.population);
        w.list(&self.buildings);
        match &self.seen_now {
            None => w.u8(0),
            Some(s) => {
                w.u8(1);
                w.u64(s.id);
                w.u16(s.hp);
            }
        }
        w.opt(&self.own);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            tile: r.u32()?,
            owner: r.u8()?,
            name: r.str()?,
            population: r.opt()?,
            buildings: r.list()?,
            seen_now: match r.u8()? {
                0 => None,
                1 => Some(SeenCity {
                    id: r.u64()?,
                    hp: r.u16()?,
                }),
                _ => return Err(WireError::Invalid("city tag")),
            },
            own: r.opt()?,
        })
    }
}

/// The seat's own player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Me {
    pub alive: bool,
    pub treasury: u32,
    pub science: u32,
    pub techs: Vec<u16>,
    pub research_queue: Vec<u16>,
}

impl Wire for Me {
    fn put(&self, w: &mut Writer) {
        w.bool(self.alive);
        w.u32(self.treasury);
        w.u32(self.science);
        w.list(&self.techs);
        w.list(&self.research_queue);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            alive: r.bool()?,
            treasury: r.u32()?,
            science: r.u32()?,
            techs: r.list()?,
            research_queue: r.list()?,
        })
    }
}

/// What every player knows of another: it lives, and the seat has met it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerPublic {
    pub alive: bool,
    pub met: bool,
}

impl Wire for PlayerPublic {
    fn put(&self, w: &mut Writer) {
        w.u8(self.alive as u8 | (self.met as u8) << 1);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let f = r.u8()?;
        if f > 3 {
            return Err(WireError::Invalid("player flags"));
        }
        Ok(Self {
            alive: f & 1 != 0,
            met: f & 2 != 0,
        })
    }
}

/// The world as one seat may know it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatView {
    pub seat: u8,
    pub round: u32,
    pub width: u32,
    pub height: u32,
    pub tiles: Vec<TileView>,
    /// By unit id.
    pub units: Vec<UnitView>,
    /// By tile.
    pub cities: Vec<CityView>,
    pub me: Me,
    /// By player id.
    pub players: Vec<PlayerPublic>,
}

impl Wire for SeatView {
    fn put(&self, w: &mut Writer) {
        w.u8(self.seat);
        w.u32(self.round);
        w.u32(self.width);
        w.u32(self.height);
        w.list(&self.tiles);
        w.list(&self.units);
        w.list(&self.cities);
        self.me.put(w);
        w.list(&self.players);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let view = Self {
            seat: r.u8()?,
            round: r.u32()?,
            width: r.u32()?,
            height: r.u32()?,
            tiles: r.list()?,
            units: r.list()?,
            cities: r.list()?,
            me: Me::take(r)?,
            players: r.list()?,
        };
        if view.tiles.len() as u64 != u64::from(view.width) * u64::from(view.height) {
            return Err(WireError::Invalid("map size"));
        }
        Ok(view)
    }
}

/// The difference of two views of one seat.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeatDelta {
    pub round: u32,
    pub me: Option<Me>,
    pub players: Option<Vec<PlayerPublic>>,
    pub tiles: Vec<(u32, TileView)>,
    pub units: Vec<UnitView>,
    pub units_gone: Vec<u64>,
    pub cities: Vec<CityView>,
    pub cities_gone: Vec<u32>,
}

impl SeatDelta {
    /// Nothing changed (the round counter apart).
    pub fn is_empty(&self) -> bool {
        self.me.is_none()
            && self.players.is_none()
            && self.tiles.is_empty()
            && self.units.is_empty()
            && self.units_gone.is_empty()
            && self.cities.is_empty()
            && self.cities_gone.is_empty()
    }
}

impl Wire for (u32, TileView) {
    fn put(&self, w: &mut Writer) {
        w.u32(self.0);
        self.1.put(w);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok((r.u32()?, TileView::take(r)?))
    }
}

impl Wire for SeatDelta {
    fn put(&self, w: &mut Writer) {
        w.u32(self.round);
        w.opt(&self.me);
        match &self.players {
            None => w.u8(0),
            Some(p) => {
                w.u8(1);
                w.list(p);
            }
        }
        w.list(&self.tiles);
        w.list(&self.units);
        w.list(&self.units_gone);
        w.list(&self.cities);
        w.list(&self.cities_gone);
    }
    fn take(r: &mut Reader<'_>) -> Result<Self, WireError> {
        Ok(Self {
            round: r.u32()?,
            me: r.opt()?,
            players: match r.u8()? {
                0 => None,
                1 => Some(r.list()?),
                _ => return Err(WireError::Invalid("players tag")),
            },
            tiles: r.list()?,
            units: r.list()?,
            units_gone: r.list()?,
            cities: r.list()?,
            cities_gone: r.list()?,
        })
    }
}

impl SeatView {
    /// The view of `seat` in `game`.
    pub fn build<M: RulesModule>(
        game: &GameState<M>,
        rules: &CompiledRuleset<M>,
        seat: PlayerId,
    ) -> Self {
        Self::build_with(game, rules, seat, false)
    }

    /// The spectators' view: the whole map open, every unit and city seen (`PlayerView::revealed`).
    /// A match shows it only when its settings say so.
    pub fn build_revealed<M: RulesModule>(game: &GameState<M>, rules: &CompiledRuleset<M>) -> Self {
        Self::build_with(game, rules, PlayerId(0), true)
    }

    fn build_with<M: RulesModule>(
        game: &GameState<M>,
        rules: &CompiledRuleset<M>,
        seat: PlayerId,
        revealed: bool,
    ) -> Self {
        let auth = game.view().auth;
        let view = game.player_view(rules, seat);
        let view = if revealed { view.revealed() } else { view };
        let dims = auth.map().dimensions;
        let tiles = (0..auth.map().tile_count() as u32)
            .map(|t| match view.tile(TileId(t)) {
                KnownTile::Unknown => TileView::Unknown,
                KnownTile::Remembered {
                    snapshot,
                    last_seen_round,
                } => TileView::Remembered {
                    snap: TileSnap::of(&snapshot, false),
                    seen_round: last_seen_round,
                },
                KnownTile::Visible(snapshot) => {
                    TileView::Visible(TileSnap::of(&snapshot, view.known_pillaged(TileId(t))))
                }
            })
            .collect();

        let mut counters = kernel::metrics::WorkCounters::default();
        let (own, foreign) = kernel::ai::observation::units(auth, &view, &mut counters);
        let mut units: Vec<UnitView> = Vec::with_capacity(own.len() + foreign.len());
        for u in &own {
            let work = auth
                .units()
                .get(u.handle)
                .is_some_and(|v| v.core.work.is_some());
            units.push(UnitView {
                id: u.id.0,
                tile: u.tile.0,
                owner: seat.0,
                kind: u.kind.0,
                hp: u.hp,
                own: Some(OwnUnit {
                    moves_left: u.moves_left,
                    moves_max: u.moves_max,
                    fortified: u.fortified,
                    can_fortify: u.can_fortify,
                    can_attack: u.can_attack,
                    experience: u.experience,
                    level: u.level,
                    attacks_made: u.attacks_made,
                    goal: u.goal.map(|t| t.0),
                    working: work,
                }),
            });
        }
        for u in &foreign {
            if let Some(core) = auth.units().get(u.handle) {
                units.push(UnitView {
                    id: core.core.id.0,
                    tile: u.tile.0,
                    owner: u.owner.0,
                    kind: u.kind.0,
                    hp: u.hp,
                    own: None,
                });
            }
        }
        units.sort_by_key(|u| u.id);
        if revealed {
            // A spectator sees every unit as another player would, and owns none.
            for u in &mut units {
                u.own = None;
            }
        }

        let mut cities: BTreeMap<u32, CityView> = BTreeMap::new();
        for (handle, core) in auth.cities().iter() {
            let mine = core.owner == seat && !revealed;
            if !mine && !view.is_visible(core.tile) {
                continue;
            }
            let hp = kernel::ai::observation::observed_city(auth, rules, handle, core).hp;
            cities.insert(
                core.tile.0,
                CityView {
                    tile: core.tile.0,
                    owner: core.owner.0,
                    name: core.name.clone(),
                    population: Some(core.population),
                    buildings: core.buildings.iter().map(|b| b.0).collect(),
                    seen_now: Some(SeenCity { id: core.id.0, hp }),
                    own: mine.then(|| OwnCity {
                        production: core.production,
                        food_times100: core.food_times100,
                        queue: core
                            .queue
                            .iter()
                            .map(|i| match i {
                                ProductionItem::Unit(k) => (0, k.0),
                                ProductionItem::Building(k) => (1, k.0),
                                ProductionItem::Project(p) => (2, p.0),
                            })
                            .collect(),
                    }),
                },
            );
        }
        for (tile, sighting) in view.remembered_cities() {
            cities.entry(tile.0).or_insert_with(|| CityView {
                tile: tile.0,
                owner: sighting.owner.0,
                name: sighting.name.clone(),
                population: sighting.seen.as_ref().map(|s| s.population),
                buildings: sighting
                    .seen
                    .as_ref()
                    .map(|s| s.buildings.clone())
                    .unwrap_or_default(),
                seen_now: None,
                own: None,
            });
        }

        let players_store = auth.players();
        let players = (0..players_store.count() as u8)
            .map(|p| PlayerPublic {
                alive: players_store.is_alive(PlayerId(p)),
                met: p == seat.0 || view.contact(PlayerId(p)).met_round.is_some(),
            })
            .collect();
        Self {
            seat: if revealed { u8::MAX } else { seat.0 },
            round: auth.turn().round,
            width: dims.width,
            height: dims.height,
            tiles,
            units,
            cities: cities.into_values().collect(),
            me: if revealed {
                Me {
                    alive: false,
                    treasury: 0,
                    science: 0,
                    techs: Vec::new(),
                    research_queue: Vec::new(),
                }
            } else {
                Me {
                    alive: players_store.is_alive(seat),
                    treasury: players_store.treasury_of(seat),
                    science: players_store.science_of(seat),
                    techs: players_store.techs_of(seat).iter().map(|t| t.0).collect(),
                    research_queue: players_store
                        .research_queue_of(seat)
                        .iter()
                        .map(|t| t.0)
                        .collect(),
                }
            },
            players,
        }
    }

    /// What changed from `self` to `next` (same seat, same map).
    pub fn diff(&self, next: &SeatView) -> SeatDelta {
        debug_assert_eq!(
            (self.seat, self.width, self.height),
            (next.seat, next.width, next.height)
        );
        let mut delta = SeatDelta {
            round: next.round,
            ..SeatDelta::default()
        };
        if self.me != next.me {
            delta.me = Some(next.me.clone());
        }
        if self.players != next.players {
            delta.players = Some(next.players.clone());
        }
        for (i, (a, b)) in self.tiles.iter().zip(&next.tiles).enumerate() {
            if a != b {
                delta.tiles.push((i as u32, *b));
            }
        }
        let before: BTreeMap<u64, &UnitView> = self.units.iter().map(|u| (u.id, u)).collect();
        let after: BTreeMap<u64, &UnitView> = next.units.iter().map(|u| (u.id, u)).collect();
        for (id, u) in &after {
            if before.get(id) != Some(u) {
                delta.units.push((*u).clone());
            }
        }
        delta.units_gone = before
            .keys()
            .filter(|id| !after.contains_key(id))
            .copied()
            .collect();
        let before: BTreeMap<u32, &CityView> = self.cities.iter().map(|c| (c.tile, c)).collect();
        let after: BTreeMap<u32, &CityView> = next.cities.iter().map(|c| (c.tile, c)).collect();
        for (tile, c) in &after {
            if before.get(tile) != Some(c) {
                delta.cities.push((*c).clone());
            }
        }
        delta.cities_gone = before
            .keys()
            .filter(|t| !after.contains_key(t))
            .copied()
            .collect();
        delta
    }

    /// The view after `delta`.
    pub fn apply(&mut self, delta: &SeatDelta) -> Result<(), WireError> {
        self.round = delta.round;
        if let Some(me) = &delta.me {
            self.me = me.clone();
        }
        if let Some(players) = &delta.players {
            self.players = players.clone();
        }
        for (i, tile) in &delta.tiles {
            *self
                .tiles
                .get_mut(*i as usize)
                .ok_or(WireError::Invalid("tile index"))? = *tile;
        }
        let mut units: BTreeMap<u64, UnitView> = std::mem::take(&mut self.units)
            .into_iter()
            .map(|u| (u.id, u))
            .collect();
        for id in &delta.units_gone {
            units.remove(id);
        }
        for u in &delta.units {
            units.insert(u.id, u.clone());
        }
        self.units = units.into_values().collect();
        let mut cities: BTreeMap<u32, CityView> = std::mem::take(&mut self.cities)
            .into_iter()
            .map(|c| (c.tile, c))
            .collect();
        for t in &delta.cities_gone {
            cities.remove(t);
        }
        for c in &delta.cities {
            cities.insert(c.tile, c.clone());
        }
        self.cities = cities.into_values().collect();
        Ok(())
    }
}
