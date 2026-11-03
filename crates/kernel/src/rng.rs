//! Deterministic RNG streams (§24 platform/project.md).
//!
//! NOT one global RNG: each mechanic has its own independent logical stream,
//! so one system's consumption does not shift another's results:
//! 100 extra combat rolls do not change world event outcomes.
//! Counter/xorshift approach: the state is only (seed, counter), persisted
//! in the save and included in the canonical hash. Wall-clock is forbidden.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamRng {
    state: u64,
}

impl StreamRng {
    pub fn new(seed: u64) -> Self {
        // A zero state degenerates xorshift, so mix with a constant.
        Self {
            state: seed.wrapping_add(0x9e3779b97f4a7c15).max(1),
        }
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x.max(1);
        self.state
    }

    /// Uniform on [0, n). Modulo bias at our n (<=10) is negligible
    /// for v1 gameplay; documented, not hidden.
    pub fn below(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        self.next_u64() % n.max(1)
    }

    pub fn state(&self) -> u64 {
        self.state
    }

    pub fn restore(state: u64) -> Self {
        Self {
            state: state.max(1),
        }
    }
}

/// The kernel's RNG streams. A new kernel mechanic with randomness gets a
/// new stream, never draws from another's; a rules module keeps its streams
/// in its own state (seeded from the game seed, saved and hashed with it).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RngState {
    pub combat: StreamRng,
    pub events: StreamRng,
}

impl RngState {
    pub fn new(seed: u64) -> Self {
        Self {
            combat: StreamRng::new(seed ^ 0xC0BA7),
            events: StreamRng::new(seed ^ 0xE7E475),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_independent() {
        // Consuming the combat stream does not shift events: the mechanics are isolated.
        let mut a = RngState::new(42);
        let mut b = RngState::new(42);
        for _ in 0..100 {
            a.combat.next_u64();
        }
        assert_eq!(a.events.next_u64(), b.events.next_u64());
        assert_ne!(a.combat.next_u64(), b.combat.next_u64());
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = StreamRng::new(7);
        let mut b = StreamRng::new(7);
        for _ in 0..50 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }
}
