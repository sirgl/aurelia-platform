//! Claiming territory tiles: culture growth (auto, in produce_city) and
//! purchase with gold (Command::PurchaseTile). Both paths go one tile at a time,
//! as in Classic (PARITY §5).
//!
//! Priority of the culture candidate (Classic border growth, with deviations):
//! resource > adjacency to a resource > river > yields > proximity > TileId.
//! Classic distinguishes luxury/strategic/bonus classes; content has none (a single
//! tier); Classic breaks ties randomly; here it is TileId (the determinism law).

use crate::command::error::SiteProblem;
use crate::module::RulesModule;
use crate::systems::ModuleSystems;
use crate::ids::{CityHandle, PlayerId, TileId};
use crate::rules::CompiledRuleset;
use crate::world::state::AuthWorld;

/// Does the tile touch a tile of this city? Purchase and culture growth
/// extend a city's own border.
fn adjacent_to_city<M: RulesModule>(auth: &AuthWorld<M>, t: TileId, city: CityHandle) -> bool {
    let dims = auth.map().dimensions;
    dims.neighbors(t)
        .into_iter()
        .any(|n| auth.map().owning_city(n) == Some(city))
}

/// May `player`'s units enter `tile`? Own and unowned land always; foreign
/// land by the module's border rule (`ModuleSystems::may_enter`). One rule
/// for path search, validation and the commit of every step.
pub fn may_enter<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId, tile: TileId) -> bool {
    let Some(host) = auth.map().owner(tile).flatten() else {
        return true;
    };
    host == player || M::Systems::may_enter(auth, player, host)
}

/// Does `host` let `guest`'s units onto its land in peace? Its own units;
/// everybody's when its kind keeps its land open
/// (`ModulePlayerKind::open_territory`); otherwise only with its
/// open-borders grant. Public: the kinds of player and the grants are
/// known to all.
pub fn lets_in<M: RulesModule>(auth: &AuthWorld<M>, host: PlayerId, guest: PlayerId) -> bool {
    use crate::world::player::ModulePlayerKind;
    let open = auth.players().kind_of(host).module().is_some_and(|kind| kind.open_territory());
    open || auth.diplomacy().grants_entry(host, guest)
}

/// `may_enter` for a unit moving by `profile`: units that enter rival
/// territory (missionaries, great people) pass closed borders. The one
/// border rule of validation, path cost and every step of a walk.
pub(crate) fn unit_may_enter<M: RulesModule>(
    auth: &AuthWorld<M>,
    profile: &crate::navigation::MoveProfile<M>,
    player: PlayerId,
    tile: TileId,
) -> bool {
    profile.rival_territory || may_enter(auth, player, tile)
}

/// Minimal city range: no city within this many tiles of another city (one tile between two
/// cities). Own value. Unciv's `minimalCityDistance` is 3, but this constant is shared by every
/// module (Lands & Laws' AI and its Colonies law settle at 2): adopting 3 for the classic module
/// needs a per-module rule and is not done.
pub const MIN_CITY_RANGE: i32 = 2;

/// Can `player` found a city on `tile` (Classic city founding)?
/// Err carries the reason shown to the player.
pub fn city_site_check<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId, tile: TileId) -> Result<(), &'static str> {
    city_site_problem(auth, player, tile).map_err(SiteProblem::english)
}

/// `city_site_check` with the reason as a type.
pub fn city_site_problem<M: RulesModule>(auth: &AuthWorld<M>, player: PlayerId, tile: TileId) -> Result<(), SiteProblem> {
    if crate::navigation::cost::enter_cost_on_map(auth.map(), tile).is_none() {
        return Err(SiteProblem::Unsettleable);
    }
    if let Some(Some(owner)) = auth.map().owner(tile) {
        if owner != player {
            return Err(SiteProblem::ForeignTerritory);
        }
    }
    let dims = auth.map().dimensions;
    for near in crate::math::tiles_in_hex_radius(&dims, tile, MIN_CITY_RANGE) {
        if auth.indexes().city_on_tile(near).is_some() {
            return Err(SiteProblem::TooClose);
        }
    }
    Ok(())
}

/// Ring of the tile relative to the city centre, if the city CAN buy it,
/// otherwise None: off the map, already owned, beyond ring 3, not adjacent to the border.
/// The single source of rules for command validate and the HUD button.
pub fn purchasable_ring<M: RulesModule>(auth: &AuthWorld<M>, city: CityHandle, tile: TileId) -> Option<i32> {
    let owner = auth.map().owner(tile)?;
    if owner.is_some() {
        return None;
    }
    let city_tile = auth.cities().get(city)?.tile;
    let dims = auth.map().dimensions;
    let dc = dims.to_coord(city_tile)?;
    let tc = dims.to_coord(tile)?;
    let ring = dc.distance(tc);
    if ring < 1 || ring > crate::world::city::PURCHASE_RADIUS_MAX as i32 {
        return None;
    }
    if !adjacent_to_city(auth, tile, city) {
        return None;
    }
    Some(ring)
}

/// A tile a city can buy: its ring from the city and the gold price.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TileOffer {
    pub ring: u8,
    pub price: u32,
}

/// Gold per ring beyond the first of a bought tile. Unciv:
/// CityExpansionManager.getGoldCostOfTile, equal (50).
pub const PLOT_BASE_COST: u32 = 50;
/// Gold added per tile the city has already claimed beyond its first ring
/// (by culture and purchase). Unciv: CityExpansionManager.getGoldCostOfTile,
/// equal (5).
pub const PLOT_COST_PER_PLOT_BOUGHT: u32 = 5;

/// The one price of a tile purchase (validate, commit and HUD). Unciv:
/// CityExpansionManager.getGoldCostOfTile, equal: `50 (d − 1) + 5 t`, d
/// the tile's distance from the city, t the tiles the city has claimed
/// beyond its first ring; then the city's TilePurchase modifiers (at
/// least 1 gold; Unciv lets a ring-1 tile cost 0).
pub fn tile_purchase_offer<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    tile: TileId,
) -> Option<TileOffer> {
    let ring = purchasable_ring(auth, city, tile)?;
    offer_at(auth, rules, city, ring)
}

/// Every tile the city may buy now with its offer, by tile (the city
/// screen's buy-a-tile overlay).
pub fn tile_purchase_offers<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
) -> Vec<(TileId, TileOffer)> {
    let Some(core) = auth.cities().get(city) else {
        return Vec::new();
    };
    let radius = crate::world::city::PURCHASE_RADIUS_MAX as i32;
    let rings: Vec<(TileId, i32)> = crate::math::tiles_in_hex_radius(&auth.map().dimensions, core.tile, radius)
        .into_iter()
        .filter_map(|t| Some((t, purchasable_ring(auth, city, t)?)))
        .collect();
    let mut offers: Vec<(TileId, TileOffer)> = rings
        .into_iter()
        .filter_map(|(tile, ring)| Some((tile, offer_at(auth, rules, city, ring)?)))
        .collect();
    offers.sort_by_key(|(tile, _)| *tile);
    offers
}

/// Tiles the city owns beyond its first ring (claimed by culture or bought).
/// Unciv: CityExpansionManager.tilesClaimed, equal.
pub fn tiles_claimed<M: RulesModule>(auth: &AuthWorld<M>, city: CityHandle) -> u32 {
    let Some(core) = auth.cities().get(city) else {
        return 0;
    };
    let dims = auth.map().dimensions;
    let ring1 = crate::math::tiles_in_hex_radius(&dims, core.tile, 1);
    crate::math::tiles_in_hex_radius(&dims, core.tile, crate::world::city::MAX_TERRITORY_RADIUS)
        .into_iter()
        .filter(|t| !ring1.contains(t) && auth.map().owning_city(*t) == Some(city))
        .count() as u32
}

/// The offer of a buyable tile at `ring`.
fn offer_at<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    city: CityHandle,
    ring: i32,
) -> Option<TileOffer> {
    let core = auth.cities().get(city)?;
    let claimed = tiles_claimed(auth, city);
    let price = PLOT_BASE_COST * (ring.max(1) as u32 - 1) + PLOT_COST_PER_PLOT_BOUGHT * claimed;
    let mods = crate::effects::city::city_modifiers(auth, rules, core);
    // × the game speed's gold percent (Unciv: getGoldCostOfTile, `goldCostModifier`).
    let price = crate::rules::speeds::SpeedValues::scale(price, mods.speed.gold_percent);
    let price = mods.adjusted_cost(crate::rules::effects::CostTarget::TilePurchase, price);
    Some(TileOffer { ring: ring as u8, price })
}

/// The player's city that buys `tile` cheapest, and the price (HUD button).
/// Every own city is considered; ties go to the earlier city in store order.
pub fn purchase_city_for<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    player: PlayerId,
    tile: TileId,
) -> Option<(CityHandle, u32)> {
    let mut best: Option<(CityHandle, u32)> = None;
    for (handle, core) in auth.cities().iter() {
        if core.owner != player {
            continue;
        }
        let Some(offer) = tile_purchase_offer(auth, rules, handle, tile) else {
            continue;
        };
        let cheaper = match best {
            Some((_, price)) => offer.price < price,
            None => true,
        };
        if cheaper {
            best = Some((handle, offer.price));
        }
    }
    best
}

/// Culture candidate: the priority tile for the city's next claim.
/// None: nothing to claim (no neutral adjacent tiles within radius 5):
/// culture accumulates in the bank until a candidate appears.
pub fn border_candidate<M: RulesModule>(
    auth: &AuthWorld<M>,
    rules: &CompiledRuleset<M>,
    owner: PlayerId,
    city: CityHandle,
) -> Option<TileId> {
    let core = auth.cities().get(city)?;
    if core.owner != owner {
        return None;
    }
    let dims = auth.map().dimensions;
    let region = crate::math::tiles_in_hex_radius(
        &dims,
        core.tile,
        crate::world::city::CULTURE_RADIUS_MAX as i32,
    );
    let center = dims.to_coord(core.tile)?;
    // Comparison key: smaller = better. The "higher is better" fields
    // are inverted to work with min_by_key.
    type Priority = (i32, i32, i32, i32, i32, i32);
    let mut best: Option<(Priority, TileId)> = None;
    for t in region {
        // Only neutral tiles on the map (Classic: culture does not push others out).
        if auth.map().owner(t).is_none() || auth.map().owner(t) != Some(None) {
            continue;
        }
        if !adjacent_to_city(auth, t, city) {
            continue;
        }
        let Some(tc) = dims.to_coord(t) else {
            continue;
        };
        let has_res = auth.map().resource(t).flatten().is_some();
        let adj_res = dims
            .neighbors(t)
            .into_iter()
            .any(|n| auth.map().resource(n).flatten().is_some());
        let river = auth.map().river(t);
        let yield_score = tile_yield_score(auth, rules, owner, t);
        let dist = center.distance(tc);
        let key = (
            i32::from(!has_res),
            i32::from(!adj_res),
            i32::from(!river),
            -yield_score,
            dist,
            t.0 as i32,
        );
        let better = match best {
            Some((k, _)) => key < k,
            None => true,
        };
        if better {
            best = Some((key, t));
        }
    }
    best.map(|(_, t)| t)
}

/// Tile score for the priority (PARITY: yields between resources and
/// proximity): every yield of the plot as the owner knows it (a fixed cost per yield point). The base, not the derived
/// cache: a commit must not depend on stale derived data.
fn tile_yield_score<M: RulesModule>(auth: &AuthWorld<M>, rules: &CompiledRuleset<M>, owner: PlayerId, t: TileId) -> i32 {
    let facts = crate::world::facts::TruthFacts::new(auth, rules, owner);
    let y = crate::derived::tile_yields::plot_yields(&facts, rules, t, &facts.techs).at_least(0);
    y.food + y.production + y.gold + y.science + y.culture + y.module
}
