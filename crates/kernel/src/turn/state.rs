//! Only turn defines the progression of the simulation (§20).
//!
//! Sequential turns (single player, `classic/docs/classic/rules.md`): a round
//! activates the alive players one by one in id order; each acts and ends
//! its turn, the next one becomes active. When the last alive player ends
//! its turn, the global round grows and the first alive player is active
//! again. The round is what the game calls "turn N" (UI, game progress).

use crate::ids::PlayerId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnPhase {
    Start,
    PlayerActions,
    Resolution,
    End,
}

#[derive(Debug, Clone)]
pub struct TurnState {
    /// Global round, 0-based. Grows once per full round of all players.
    pub round: u32,
    /// The player whose turn it is; only this player may act.
    pub active_player: PlayerId,
    /// Phase of the active player's turn.
    pub phase: TurnPhase,
    /// Players who may act now although it is not their turn: the open
    /// window of a simultaneous round (`GameState::open_window`), one bit
    /// per player id. Not part of the world: it is never saved or hashed
    /// (the command journal records when it opens and closes).
    pub(crate) window: u64,
    /// The seat whose command is being executed in the window (set only
    /// inside `GameState::execute_as`): `AuthWorld::active_player` answers
    /// with it, so every module's "it is your turn" check applies to the
    /// seat unchanged.
    pub(crate) acting: Option<PlayerId>,
}

/// Who acts after the current player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NextTurn {
    /// Another player of the same round.
    SameRound(PlayerId),
    /// The round is over: a new round starts with this player.
    NewRound(PlayerId),
    /// Nobody is alive.
    Nobody,
}

impl TurnState {
    pub fn new() -> Self {
        Self {
            round: 0,
            active_player: PlayerId(0),
            phase: TurnPhase::PlayerActions,
            window: 0,
            acting: None,
        }
    }

    /// The first alive player after `current` in id order, wrapping into a
    /// new round. `alive(p)` answers for every player id below `players`.
    pub(crate) fn next_after(
        current: PlayerId,
        players: usize,
        alive: impl Fn(PlayerId) -> bool,
    ) -> NextTurn {
        for p in (current.0 as usize + 1)..players {
            if alive(PlayerId(p as u8)) {
                return NextTurn::SameRound(PlayerId(p as u8));
            }
        }
        for p in 0..players {
            if alive(PlayerId(p as u8)) {
                return NextTurn::NewRound(PlayerId(p as u8));
            }
        }
        NextTurn::Nobody
    }

    /// Hand the turn over (after the active player's end-turn sequence, or
    /// when the active player was eliminated).
    pub(crate) fn pass_to(&mut self, next: NextTurn) {
        match next {
            NextTurn::SameRound(player) => self.active_player = player,
            NextTurn::NewRound(player) => {
                self.round += 1;
                self.active_player = player;
            }
            NextTurn::Nobody => {}
        }
        self.phase = TurnPhase::PlayerActions;
    }

    /// The players who may act in the open window (bit `n`: player `n`);
    /// 0: no window, only the active player acts.
    pub fn window(&self) -> u64 {
        self.window
    }

    /// A command is being executed for a seat of the open window.
    pub fn is_acting(&self) -> bool {
        self.acting.is_some()
    }

    /// The end-turn sequence walks Start → Resolution → End and returns to
    /// PlayerActions (the rules module's `TurnSchedule`).
    pub fn enter(&mut self, phase: TurnPhase) {
        self.phase = phase;
    }

    /// Restore from the save schema (the discriminant is checked in decode before the call).
    pub(crate) fn restore(round: u32, phase: TurnPhase, active_player: PlayerId) -> Self {
        Self {
            round,
            active_player,
            phase,
            window: 0,
            acting: None,
        }
    }
}

impl Default for TurnState {
    fn default() -> Self {
        Self::new()
    }
}
