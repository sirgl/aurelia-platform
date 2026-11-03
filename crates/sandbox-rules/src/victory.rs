//! The one victory, Dominion: a player who holds every original capital
//! wins at once; when the round limit comes first, the largest empire wins
//! (most cities, then most citizens, then the lower player id). Read from
//! the kernel's world alone, so the sandbox keeps no victory state.

use kernel::ids::PlayerId;
use kernel::world::state::AuthWorld;

use crate::Sandbox;

/// The round the game ends at if nobody holds every capital before.
pub const ROUND_LIMIT: u32 = 100;

/// The winner, once there is one.
pub fn victor(world: &AuthWorld<Sandbox>) -> Option<PlayerId> {
    capitals_holder(world).or_else(|| (world.turn().round >= ROUND_LIMIT).then(|| largest_empire(world)).flatten())
}

/// The one owner of every original capital (at least two were founded).
pub fn capitals_holder(world: &AuthWorld<Sandbox>) -> Option<PlayerId> {
    let mut owners = world.cities().iter().filter(|(_, c)| c.original_capital).map(|(_, c)| c.owner);
    let first = owners.next()?;
    let mut count = 1;
    for owner in owners {
        if owner != first {
            return None;
        }
        count += 1;
    }
    (count >= 2).then_some(first)
}

/// The living player with the most cities, then the most citizens.
pub fn largest_empire(world: &AuthWorld<Sandbox>) -> Option<PlayerId> {
    let players = world.players();
    (0..players.count() as u8)
        .map(PlayerId)
        .filter(|p| players.is_alive(*p))
        .max_by_key(|p| (score(world, *p), std::cmp::Reverse(p.0)))
}

/// (cities, citizens) of a player.
pub fn score(world: &AuthWorld<Sandbox>, player: PlayerId) -> (usize, u32) {
    let cities = world.indexes().cities_of(player);
    let citizens = cities.iter().filter_map(|c| world.cities().get(*c)).map(|c| c.population).sum();
    (cities.len(), citizens)
}
