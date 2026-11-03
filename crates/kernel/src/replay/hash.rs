//! Canonical hash of the authoritative state only (§25 platform/project.md).
//!
//! Coverage v2: map (all 6 layers) + units + cities + diplomacy + turn +
//! players + knowledge, and the rules module's state at its `HashPoint`s. Excluded: derived caches, renderer, AI tmp, scratch.
//! The traversal order is stable: units and cities in entity order (ids
//! mixed), matrices row-major.
//! HashMap indexes are NOT hashed (iteration order is non-deterministic;
//! their consistency is checked by WorldValidator).

use crate::module::RulesModule;
use crate::ids::TileId;
use crate::turn::TurnPhase;
use crate::world::module::{HashPoint, ModuleUnit, ModuleWorld};
use crate::world::state::GameState;

fn mix(h: &mut u64, v: u64) {
    *h ^= v;
    *h = h.wrapping_mul(0x100000001b3);
}

const FNV_PRIME: u64 = 0x100000001b3;
/// `FNV_PRIME` to the 64th power: 64 `mix(h, 0)` in a row are one multiplication by it.
const FNV_PRIME_64: u64 = {
    let mut p = 1u64;
    let mut i = 0;
    while i < 64 {
        p = p.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    p
};

/// Per tile of the player's knowledge: the explored flag and, for an explored tile, its memory.
/// A word of 64 unexplored tiles is 64 `mix(h, 0)`, done as one multiplication (the same hash).
fn mix_memories(h: &mut u64, k: &crate::knowledge::PlayerKnowledge) {
    let memories = k.memories();
    let words = k.explored_tiles().words();
    let tiles = memories.len();
    let mut tile = 0;
    while tile < tiles {
        let word = words.get(tile / 64).copied().unwrap_or(0);
        if tile % 64 == 0 && tile + 64 <= tiles && word == 0 {
            *h = h.wrapping_mul(FNV_PRIME_64);
            tile += 64;
            continue;
        }
        let explored = (word >> (tile % 64)) & 1 == 1;
        mix(h, u64::from(explored));
        if explored {
            if let Some(memory) = memories.get(tile) {
                let (round, improvement, feature, owner, flags) = memory.packed();
                let packed = (round as u64) << 32 | (improvement as u64) << 16 | feature as u64;
                mix(h, packed ^ ((owner as u64) << 56) ^ ((flags as u64) << 48));
                mix(h, memory.resource_code() as u64);
            }
        }
        tile += 1;
    }
}

fn phase_disc(p: TurnPhase) -> u64 {
    match p {
        TurnPhase::Start => 0,
        TurnPhase::PlayerActions => 1,
        TurnPhase::Resolution => 2,
        TurnPhase::End => 3,
    }
}

pub fn authoritative_hash<M: RulesModule>(game: &GameState<M>) -> u64 {
    let view = game.view();
    let auth = view.auth;
    let mut h: u64 = 0xcbf29ce484222325;

    // Turn: number + phase + active player.
    mix(&mut h, auth.turn().round as u64);
    mix(&mut h, phase_disc(auth.turn().phase));
    mix(&mut h, auth.active_player().0 as u64);

    // Players: alive + techs + treasury + science + research queue.
    // All of it is truth: it gates commands, yields, hurry and tech completion.
    // The only place players are hashed (no duplicates).
    for i in 0..auth.players().count() {
        let p = crate::ids::PlayerId(i as u8);
        mix(&mut h, u64::from(auth.players().is_alive(p)));
        for t in auth.players().techs_of(p) {
            mix(&mut h, t.0 as u64);
        }
        mix(&mut h, auth.players().treasury_of(p) as u64);
        mix(&mut h, auth.players().science_of(p) as u64);
        mix(&mut h, u64::from(auth.players().nation_of(p)));
        // Kind: mixed only when not a major (games of majors only hash as
        // before M4).
        let kind = auth.players().kind_of(p);
        if kind != crate::world::player::PlayerKind::Major {
            let (tag, id) = kind.code();
            mix(&mut h, 0x6b69_6e64 ^ ((tag as u64) << 16) ^ id as u64);
        }
        auth.module().hash(HashPoint::Player(p), auth, &mut |v| mix(&mut h, v));
        for t in auth.players().research_queue_of(p) {
            mix(&mut h, t.0 as u64);
        }
        mix(&mut h, 0x85ebca6b);
    }

    // Map: all layers in TileId order.
    let n = auth.map().tile_count();
    let city_ordinals = crate::persistence::encode::city_ordinals(auth);
    for i in 0..n {
        let t = TileId(i as u32);
        mix(
            &mut h,
            auth.map()
                .terrain(t)
                .map(|x| x.0 as u64)
                .unwrap_or(u64::MAX),
        );
        mix(
            &mut h,
            auth.map()
                .feature(t)
                .map(|o| o.map(|x| x.0 as u64).unwrap_or(u64::MAX))
                .unwrap_or(u64::MAX - 1),
        );
        mix(
            &mut h,
            auth.map()
                .resource(t)
                .map(|o| o.map(|x| x.0 as u64).unwrap_or(u64::MAX))
                .unwrap_or(u64::MAX - 1),
        );
        mix(
            &mut h,
            auth.map()
                .owner(t)
                .map(|o| o.map(|p| p.0 as u64).unwrap_or(u64::MAX))
                .unwrap_or(u64::MAX - 1),
        );
        mix(
            &mut h,
            auth.map()
                .improvement(t)
                .map(|o| o.map(|x| x.0 as u64).unwrap_or(u64::MAX))
                .unwrap_or(u64::MAX - 1),
        );
        // Deposits of several copies only: single resources keep the hash.
        let amount = auth.map().resource_amount(t);
        if amount > 1 {
            mix(&mut h, 0x5245_5300 | u64::from(amount));
        }
        // Route code: 0/1 as the former road flag, so hashes of road-only maps hold.
        mix(&mut h, u64::from(auth.map().route(t).code()));
        // Only pillaged tiles mix: maps without pillage keep their hash.
        if auth.map().pillaged(t) {
            mix(&mut h, 0x9111_A6ED);
        }
        // Mixed only where present: maps without natural wonders keep their hashes.
        if let Some(wonder) = auth.map().natural_wonder(t) {
            mix(&mut h, 0x4E57_0000 | u64::from(wonder.0));
        }
        mix(&mut h, u64::from(auth.map().river(t)));
        // Owning city (position in store order), mixed only on owned tiles.
        if let Some(city) = auth.map().owning_city(t) {
            mix(&mut h, 0x6369_7479 + city_ordinals[city.index as usize] as u64);
        }
    }
    // River beds: canonical pairs (sorted, so deterministic).
    for (a, b) in auth.map().river_edges() {
        mix(&mut h, *a as u64);
        mix(&mut h, *b as u64);
    }

    // Units in entity order with their persistent ids (handles are runtime
    // identity and are NOT hashed, §17 platform/structure.md).
    mix(&mut h, auth.next_entity().0);
    for (handle, core) in auth.units().iter_handles() {
        mix(&mut h, core.id.0);
        mix(&mut h, core.tile.0 as u64);
        mix(&mut h, core.owner.0 as u64);
        mix(&mut h, core.kind.0 as u64);
        mix(&mut h, core.hp as u64);
        mix(
            &mut h,
            ((core.moves_left as u64) << 32) | core.moves_max as u64,
        );
        let (stance, turns) = crate::persistence::encode::encode_stance(core.stance);
        mix(&mut h, u64::from(stance) | (u64::from(turns) << 8));
        // Combat state: mixed only when it differs from a new unit's.
        if core.experience != 0 || core.level != 1 {
            mix(&mut h, 0x7870_6c76 ^ ((core.experience as u64) << 8) ^ core.level as u64);
        }
        if core.attacks_made != 0 || core.interceptions_made != 0 {
            mix(&mut h, 0x6174_6b73 ^ ((core.attacks_made as u64) << 8) ^ core.interceptions_made as u64);
        }
        if let Some(w) = core.work {
            mix(&mut h, w.tile.0 as u64);
            match w.target {
                crate::ids::WorkTarget::Improvement(imp) => {
                    mix(&mut h, 701);
                    mix(&mut h, imp.0 as u64);
                }
                crate::ids::WorkTarget::Road => {
                    mix(&mut h, 702);
                }
                crate::ids::WorkTarget::Chop(feat) => {
                    mix(&mut h, 703);
                    mix(&mut h, feat.0 as u64);
                }
            }
            mix(&mut h, w.work_left as u64);
        }
        if let Some(goal) = core.goal {
            mix(&mut h, 0x676f_616c);
            mix(&mut h, goal.0 as u64);
        }
        // Promotions: mixed only when present (a unit without promotions
        // hashes as before the field existed).
        for p in core.promotions.iter() {
            mix(&mut h, 0x70726f6d);
            mix(&mut h, p.0 as u64);
        }
        // The module's state of the unit (a module without mixes nothing).
        if let Some(state) = auth.units().module_of(handle) {
            state.hash(&mut |v| mix(&mut h, v));
        }
        mix(&mut h, 0x8badf00d);
    }

    // Cities: tile/owner/name/pop/food/buildings/production/queue.
    for (_, core) in auth.cities().iter() {
        mix(&mut h, core.id.0);
        mix(&mut h, core.tile.0 as u64);
        mix(&mut h, core.owner.0 as u64);
        mix(&mut h, core.name.len() as u64);
        for b in core.name.as_bytes() {
            mix(&mut h, *b as u64);
        }
        mix(&mut h, core.population as u64);
        mix(&mut h, core.food_times100 as u64);
        for t in &core.unworked {
            mix(&mut h, t.0 as u64);
        }
        mix(&mut h, core.culture as u64);
        mix(&mut h, core.acquired as u64);
        mix(&mut h, core.radius as u64);
        mix(&mut h, core.damage as u64);
        mix(&mut h, u64::from(core.acquired_round));
        let status = crate::persistence::encode::encode_status(core.status);





        let conquest = core.original_owner != core.owner
            || core.original_capital
            || status != 0
            || core.resistance_turns != 0
            || core.strike_made;
        if conquest {
            mix(&mut h, 0x636f_6e71);
            mix(&mut h, core.original_owner.0 as u64 | (u64::from(core.original_capital) << 8));
            mix(&mut h, status as u64 | (core.resistance_turns as u64) << 8 | (u64::from(core.strike_made) << 32));
        }
        if core.capital_connection == crate::world::city::CapitalConnection::Connected {
            mix(&mut h, 0x6361_706c);
        }
        for b in &core.buildings {
            mix(&mut h, b.0 as u64);
        }
        mix(&mut h, core.production as u64);
        for q in &core.queue {
            match q {
                crate::ids::ProductionItem::Unit(k) => {
                    mix(&mut h, 0x75_6e_69_74);
                    mix(&mut h, k.0 as u64);
                }
                crate::ids::ProductionItem::Building(b) => {
                    mix(&mut h, 0x62_6c_64_67);
                    mix(&mut h, b.0 as u64);
                }
                crate::ids::ProductionItem::Project(p) => {
                    mix(&mut h, 0x70_72_6a);
                    mix(&mut h, p.0 as u64);
                }
            }
        }
        mix(&mut h, 0x9e3779b9);
    }

    // Diplomacy: raw row-major matrices (war + open borders).
    for b in auth.diplomacy().matrix() {
        mix(&mut h, *b as u64);
    }
    for b in auth.diplomacy().open_matrix() {
        mix(&mut h, *b as u64);
    }
    auth.module().hash(HashPoint::Diplomacy, auth, &mut |v| mix(&mut h, v));

    // Timed effects, store order (an empty store adds nothing).
    let city_order: Vec<_> = auth.cities().iter().map(|(handle, _)| handle).collect();
    for entry in auth.active_effects().iter() {
        mix(&mut h, 0x7469_6d65);
        mix(&mut h, entry.owner.0 as u64);
        match entry.target {
            crate::world::effects::EffectTarget::Player(p) => mix(&mut h, p.0 as u64),
            crate::world::effects::EffectTarget::Units(p) => mix(&mut h, 0x2_0000 + p.0 as u64),
            crate::world::effects::EffectTarget::City(c) => {
                let ordinal = city_order
                    .iter()
                    .position(|x| *x == c)
                    .unwrap_or(usize::MAX);
                mix(&mut h, 0x1_0000 + ordinal as u64);
            }
        }
        let (tag, id) = crate::persistence::effects::source_parts(entry.effect.source);
        mix(&mut h, ((tag as u64) << 16) | id as u64);
        mix(&mut h, entry.effect.trigger.index() as u64);
        mix(&mut h, entry.effect.slot as u64);
        mix(&mut h, entry.turns_left as u64);
    }

    // Economy and society section (track M4-A).
    mix(&mut h, crate::persistence::economy::hash(auth));
    auth.module().hash(HashPoint::AfterEconomy, auth, &mut |v| mix(&mut h, v));

    // RNG is part of the truth (§24): a stream desync must change the hash.
    mix(&mut h, auth.rng().combat.state());
    mix(&mut h, auth.rng().events.state());
    auth.module().hash(HashPoint::AfterRng, auth, &mut |v| mix(&mut h, v));

    // Knowledge (§45: real state, saved): explored flags, the memory of
    // explored tiles, contacts.
    for k in auth.knowledge() {
        mix_memories(&mut h, k);
        mix(&mut h, k.sightings().count() as u64);
        for (tile, city) in k.sightings() {
            mix(&mut h, ((tile.0 as u64) << 8) | city.owner.0 as u64);
            mix(&mut h, city.name.len() as u64);
            city.name.bytes().for_each(|b| mix(&mut h, b as u64));
            match &city.seen {
                None => mix(&mut h, u64::MAX),
                Some(seen) => {
                    mix(&mut h, ((seen.population as u64) << 8) | seen.era as u64);
                    mix(&mut h, seen.buildings.len() as u64);
                    seen.buildings.iter().for_each(|b| mix(&mut h, *b as u64));
                }
            }
        }
        for contact in k.contacts() {
            mix(&mut h, contact.met_round.map(|r| r as u64 + 1).unwrap_or(0));
            mix(&mut h, contact.last_seen_round.map(|r| r as u64 + 1).unwrap_or(0));
        }
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sixty_four_zero_mixes_are_one_multiplication() {
        for start in [0u64, 1, 0xcbf2_9ce4_8422_2325, u64::MAX] {
            let mut h = start;
            for _ in 0..64 {
                mix(&mut h, 0);
            }
            assert_eq!(h, start.wrapping_mul(FNV_PRIME_64));
        }
    }
}
