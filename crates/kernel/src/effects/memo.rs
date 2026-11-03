//! A memo of effect results that is valid while the world does not change.
//!
//! `player_modifiers`, the connected-city count and a module's follower
//! counts are pure functions of the authoritative world, and the AI, the
//! derived state and the HUD ask for them thousands of times a round between
//! two commands. The memo answers the repeated asks.
//!
//! **Validity.** The memo is open (`period != 0`) only while nobody can
//! change the world: a mutation needs `&mut GameState`, and every such path
//! (`execute`, `world_mut`, the harness `debug_*` methods) closes the memo
//! before it touches the world. A shared read (`GameState::view`,
//! `player_view`, `derived`) opens it again with a fresh period id, and an
//! entry from an earlier period is never returned. During a command the
//! memo stays closed: the rules module changes the world through public
//! fields, so there is no point at which a result computed inside the
//! command is known to be current.
//!
//! **Oracle.** `MemoMode::Verify` recomputes on every hit and asserts that
//! the memo agrees (`AURELIA_MEMO=verify` in the environment, or
//! `GameState::set_memo_mode`); `MemoMode::Off` never stores (the
//! differential tests play a game in both modes and compare world hashes).
//!
//! A clone of the world starts with a closed, empty memo.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use crate::effects::player::PlayerModifiers;
use crate::ids::{CityHandle, PlayerId};
use crate::module::RulesModule;

/// How the memo answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoMode {
    /// Never stores, always computes.
    Off,
    /// Stores and answers from the store.
    On,
    /// As `On`, and recomputes every answer from the store and asserts that
    /// the two agree.
    Verify,
}

impl MemoMode {
    fn code(self) -> u8 {
        match self {
            MemoMode::Off => 0,
            MemoMode::On => 1,
            MemoMode::Verify => 2,
        }
    }

    fn of_code(code: u8) -> Self {
        match code {
            0 => MemoMode::Off,
            1 => MemoMode::On,
            _ => MemoMode::Verify,
        }
    }

    /// The mode a new world starts in: `AURELIA_MEMO=off|verify`, else `On`
    /// (`Verify` in a debug build with `AURELIA_MEMO=verify` only: the oracle is
    /// slow, so it is never the default).
    pub fn from_environment() -> Self {
        static MODE: OnceLock<MemoMode> = OnceLock::new();
        *MODE.get_or_init(|| match std::env::var("AURELIA_MEMO").as_deref() {
            Ok("off") => MemoMode::Off,
            Ok("verify") => MemoMode::Verify,
            _ => MemoMode::On,
        })
    }
}

/// Ids of the open periods; unique over every world of the process, so a
/// clone that diverges never meets an entry of its original.
static NEXT_PERIOD: AtomicU64 = AtomicU64::new(1);

/// Counters of the memo, for the tests and the profile.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MemoStats {
    pub hits: u64,
    pub misses: u64,
}

struct State<M: RulesModule> {
    /// The period the entries belong to.
    period: u64,
    players: Vec<Option<Arc<PlayerModifiers<M>>>>,
    connected: Vec<Option<(CityHandle, Arc<Vec<CityHandle>>)>>,
    /// The module's own memo tables: one `HashMap<K, V>` per (K, V).
    module: HashMap<TypeId, Box<dyn Any + Send>>,
    stats: MemoStats,
}

impl<M: RulesModule> State<M> {
    fn new() -> Self {
        Self {
            period: 0,
            players: Vec::new(),
            connected: Vec::new(),
            module: HashMap::new(),
            stats: MemoStats::default(),
        }
    }

    fn enter(&mut self, period: u64) {
        if self.period != period {
            self.period = period;
            self.players.clear();
            self.connected.clear();
            self.module.clear();
        }
    }
}

pub struct EffectMemo<M: RulesModule> {
    /// 0: closed. Otherwise the id of the open period.
    period: AtomicU64,
    mode: AtomicU8,
    state: Mutex<State<M>>,
}

impl<M: RulesModule> Default for EffectMemo<M> {
    fn default() -> Self {
        Self {
            period: AtomicU64::new(0),
            mode: AtomicU8::new(MemoMode::from_environment().code()),
            state: Mutex::new(State::new()),
        }
    }
}

impl<M: RulesModule> Clone for EffectMemo<M> {
    fn clone(&self) -> Self {
        let clone = Self::default();
        clone
            .mode
            .store(self.mode.load(Ordering::Relaxed), Ordering::Relaxed);
        clone
    }
}

impl<M: RulesModule> fmt::Debug for EffectMemo<M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "EffectMemo({:?}, open: {})",
            self.mode(),
            self.period.load(Ordering::Relaxed) != 0
        )
    }
}

impl<M: RulesModule> EffectMemo<M> {
    pub fn mode(&self) -> MemoMode {
        MemoMode::of_code(self.mode.load(Ordering::Relaxed))
    }

    pub fn set_mode(&self, mode: MemoMode) {
        self.mode.store(mode.code(), Ordering::Relaxed);
        self.close();
    }

    /// The world is about to change: no entry is valid any more.
    #[inline]
    pub fn close(&self) {
        self.period.store(0, Ordering::Release);
    }

    /// Nobody can change the world until the next `close`: entries made from
    /// here on are valid. Does nothing when already open.
    #[inline]
    pub fn open(&self) {
        if self.period.load(Ordering::Acquire) == 0 {
            let id = NEXT_PERIOD.fetch_add(1, Ordering::Relaxed);
            self.period.store(id, Ordering::Release);
        }
    }

    /// Hits and misses so far (all periods).
    pub fn stats(&self) -> MemoStats {
        self.lock().stats
    }

    fn lock(&self) -> MutexGuard<'_, State<M>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The open period and the mode when answering from the memo is on.
    #[inline]
    fn active(&self) -> Option<(u64, MemoMode)> {
        let period = self.period.load(Ordering::Acquire);
        if period == 0 {
            return None;
        }
        match self.mode() {
            MemoMode::Off => None,
            mode => Some((period, mode)),
        }
    }

    /// `player`'s modifiers: from the memo, else `compute`d.
    pub fn player_modifiers(
        &self,
        player: PlayerId,
        compute: impl FnOnce() -> PlayerModifiers<M>,
    ) -> Arc<PlayerModifiers<M>> {
        let Some((period, mode)) = self.active() else {
            return Arc::new(compute());
        };
        let slot = player.0 as usize;
        let found = {
            let mut state = self.lock();
            state.enter(period);
            let found = state.players.get(slot).and_then(|e| e.clone());
            if found.is_some() {
                state.stats.hits += 1;
            } else {
                state.stats.misses += 1;
            }
            found
        };
        if let Some(found) = found {
            if mode == MemoMode::Verify {
                let fresh = compute();
                assert_eq!(*found, fresh, "stale memo: player_modifiers of {player:?}");
            }
            return found;
        }
        let fresh = Arc::new(compute());
        let mut state = self.lock();
        // The world cannot have changed while `compute` ran (it holds a
        // shared borrow), but a recursive call may have filled the slot.
        if state.period == period {
            if state.players.len() <= slot {
                state.players.resize(slot + 1, None);
            }
            state.players[slot] = Some(fresh.clone());
        }
        fresh
    }

    /// The cities connected to `capital` of `player`.
    pub fn connected_cities(
        &self,
        player: PlayerId,
        capital: CityHandle,
        compute: impl FnOnce() -> Vec<CityHandle>,
    ) -> Arc<Vec<CityHandle>> {
        let Some((period, mode)) = self.active() else {
            return Arc::new(compute());
        };
        let slot = player.0 as usize;
        let found = {
            let mut state = self.lock();
            state.enter(period);
            let found = state
                .connected
                .get(slot)
                .and_then(|e| e.as_ref())
                .filter(|(cap, _)| *cap == capital)
                .map(|(_, list)| list.clone());
            if found.is_some() {
                state.stats.hits += 1;
            } else {
                state.stats.misses += 1;
            }
            found
        };
        if let Some(found) = found {
            if mode == MemoMode::Verify {
                let fresh = compute();
                assert_eq!(*found, fresh, "stale memo: connected_cities of {player:?}");
            }
            return found;
        }
        let fresh = Arc::new(compute());
        let mut state = self.lock();
        if state.period == period {
            if state.connected.len() <= slot {
                state.connected.resize(slot + 1, None);
            }
            state.connected[slot] = Some((capital, fresh.clone()));
        }
        fresh
    }

    /// A value of the rules module's own, `V` for a key `K`. Each (K, V)
    /// pair has a table of its own: use a private key type per memoised
    /// function.
    pub fn module_value<K, V>(&self, key: K, compute: impl FnOnce() -> V) -> V
    where
        K: Copy + Eq + Hash + Send + 'static,
        V: Clone + PartialEq + fmt::Debug + Send + 'static,
    {
        let Some((period, mode)) = self.active() else {
            return compute();
        };
        let table = TypeId::of::<(K, V)>();
        let found = {
            let mut state = self.lock();
            state.enter(period);
            let found = state
                .module
                .get(&table)
                .and_then(|t| t.downcast_ref::<HashMap<K, V>>())
                .and_then(|t| t.get(&key))
                .cloned();
            if found.is_some() {
                state.stats.hits += 1;
            } else {
                state.stats.misses += 1;
            }
            found
        };
        if let Some(found) = found {
            if mode == MemoMode::Verify {
                let fresh = compute();
                assert_eq!(found, fresh, "stale memo: module value");
            }
            return found;
        }
        let fresh = compute();
        let mut state = self.lock();
        if state.period == period {
            let entry = state
                .module
                .entry(table)
                .or_insert_with(|| Box::new(HashMap::<K, V>::new()));
            if let Some(table) = entry.downcast_mut::<HashMap<K, V>>() {
                table.insert(key, fresh.clone());
            }
        }
        fresh
    }
}
