//! The observer's time: pause, the speeds and the step of one round. Speed is only the
//! pause between rounds (and how much of a frame the rounds may take): the rounds
//! themselves are played by the same AI steps whatever the speed, so the match's hash
//! does not depend on it. Pure and clock-free: the caller passes the time that passed.
use std::time::Duration;

/// The pause between two rounds at ×1.
pub const ROUND_PAUSE: Duration = Duration::from_millis(1000);
/// What a frame spends on the AI's steps at the speeds with a pause.
pub const SLICE: Duration = Duration::from_millis(4);
/// What a frame spends on them at the greatest speed.
pub const MAX_SLICE: Duration = Duration::from_millis(14);

/// How fast the rounds follow each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Speed {
    Quarter,
    Half,
    One,
    Two,
    Four,
    Eight,
    Sixteen,
    /// No pause: a round starts as soon as the last one is shown.
    Max,
}

impl Speed {
    pub const ALL: [Speed; 8] = [Speed::Quarter, Speed::Half, Speed::One, Speed::Two, Speed::Four, Speed::Eight, Speed::Sixteen, Speed::Max];

    pub fn index(self) -> usize {
        Self::ALL.iter().position(|s| *s == self).unwrap_or(2)
    }

    pub fn faster(self) -> Speed {
        Self::ALL[(self.index() + 1).min(Self::ALL.len() - 1)]
    }

    pub fn slower(self) -> Speed {
        Self::ALL[self.index().saturating_sub(1)]
    }

    /// The pause between rounds.
    pub fn pause(self) -> Duration {
        match self {
            Speed::Quarter => ROUND_PAUSE * 4,
            Speed::Half => ROUND_PAUSE * 2,
            Speed::One => ROUND_PAUSE,
            Speed::Two => ROUND_PAUSE / 2,
            Speed::Four => ROUND_PAUSE / 4,
            Speed::Eight => ROUND_PAUSE / 8,
            Speed::Sixteen => ROUND_PAUSE / 16,
            Speed::Max => Duration::ZERO,
        }
    }

    /// The wall time a frame gives the AI's steps.
    pub fn slice(self) -> Duration {
        if self == Speed::Max {
            MAX_SLICE
        } else {
            SLICE
        }
    }
}

/// Whether the clock runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    Running,
    Paused,
}

/// Whether a round is being played or shown, so that no new one may start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    Free,
    Held,
}

/// What the clock asks for in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Nothing starts.
    Wait,
    /// A new round starts.
    Round,
}

/// Pause, speed and the time waited since the last round ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    paused: bool,
    speed: Speed,
    waited: Duration,
    step: bool,
}

impl Default for Clock {
    fn default() -> Self {
        Self::new(Speed::One)
    }
}

impl Clock {
    /// A running clock at `speed`.
    pub fn new(speed: Speed) -> Self {
        Self { paused: false, speed, waited: Duration::ZERO, step: false }
    }

    /// A clock that starts paused.
    pub fn paused(speed: Speed) -> Self {
        Self { paused: true, ..Self::new(speed) }
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn pace(&self) -> Pace {
        if self.paused {
            Pace::Paused
        } else {
            Pace::Running
        }
    }

    pub fn speed(&self) -> Speed {
        self.speed
    }

    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }

    pub fn faster(&mut self) {
        self.speed = self.speed.faster();
    }

    pub fn slower(&mut self) {
        self.speed = self.speed.slower();
    }

    pub fn set_speed(&mut self, speed: Speed) {
        self.speed = speed;
    }

    /// One round on the next tick, whatever the pause (the step key). Only a paused
    /// clock steps; a running one plays the round anyway.
    pub fn step(&mut self) {
        if self.paused {
            self.step = true;
        }
    }

    /// A step is waiting for its tick.
    pub fn step_pending(&self) -> bool {
        self.step
    }

    /// `elapsed` passed since the last tick. `hold`: a round is being played or shown, so
    /// no new one starts and the pause does not run.
    pub fn tick(&mut self, elapsed: Duration, hold: Hold) -> Order {
        if hold == Hold::Held {
            return Order::Wait;
        }
        if std::mem::take(&mut self.step) {
            self.waited = Duration::ZERO;
            return Order::Round;
        }
        if self.paused {
            return Order::Wait;
        }
        self.waited += elapsed;
        if self.waited >= self.speed.pause() {
            self.waited = Duration::ZERO;
            return Order::Round;
        }
        Order::Wait
    }

    /// The time left of the pause, for a window that sleeps until it ends (None:
    /// paused, or no pause).
    pub fn until_next(&self) -> Option<Duration> {
        (!self.paused).then(|| self.speed.pause().saturating_sub(self.waited))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    fn rounds_in(clock: &mut Clock, frames: u32) -> u32 {
        (0..frames).filter(|_| clock.tick(FRAME, Hold::Free) == Order::Round).count() as u32
    }

    #[test]
    fn speeds_double_from_a_quarter_to_sixteen_then_max() {
        let pauses: Vec<u128> = Speed::ALL.iter().map(|s| s.pause().as_millis()).collect();
        assert_eq!(pauses, [4000, 2000, 1000, 500, 250, 125, 62, 0]);
        assert_eq!(Speed::Quarter.slower(), Speed::Quarter);
        assert_eq!(Speed::Max.faster(), Speed::Max);
        assert_eq!(Speed::One.faster(), Speed::Two);
        assert_eq!(Speed::One.slower(), Speed::Half);
        assert!(Speed::Max.slice() > Speed::One.slice());
    }

    #[test]
    fn a_running_clock_starts_a_round_after_each_pause() {
        let mut clock = Clock::new(Speed::One);
        assert_eq!(rounds_in(&mut clock, 130), 2, "130 frames of 16 ms hold two pauses");
        let mut fast = Clock::new(Speed::Eight);
        assert!(rounds_in(&mut fast, 125) > 8 * 1);
    }

    #[test]
    fn a_busy_clock_neither_starts_a_round_nor_runs_the_pause() {
        let mut clock = Clock::new(Speed::One);
        for _ in 0..1000 {
            assert_eq!(clock.tick(FRAME, Hold::Held), Order::Wait);
        }
        assert_eq!(clock.tick(Duration::from_millis(900), Hold::Free), Order::Wait, "the pause starts when the round is shown");
    }

    #[test]
    fn a_paused_clock_plays_only_the_steps_asked_for() {
        let mut clock = Clock::paused(Speed::Max);
        assert_eq!(rounds_in(&mut clock, 100), 0);
        clock.step();
        assert!(clock.step_pending());
        assert_eq!(clock.tick(FRAME, Hold::Free), Order::Round);
        assert_eq!(clock.tick(FRAME, Hold::Free), Order::Wait);
        clock.step();
        clock.step();
        assert_eq!(rounds_in(&mut clock, 3), 1, "two presses before a tick are one step");
    }

    #[test]
    fn a_step_waits_for_the_round_in_progress() {
        let mut clock = Clock::paused(Speed::One);
        clock.step();
        assert_eq!(clock.tick(FRAME, Hold::Held), Order::Wait);
        assert_eq!(clock.tick(FRAME, Hold::Free), Order::Round, "the press is kept until the round is shown");
    }

    #[test]
    fn a_running_clock_ignores_the_step_key() {
        let mut clock = Clock::new(Speed::Quarter);
        clock.step();
        assert!(!clock.step_pending());
        assert_eq!(clock.tick(FRAME, Hold::Free), Order::Wait);
    }

    #[test]
    fn max_speed_starts_a_round_on_every_free_tick() {
        let mut clock = Clock::new(Speed::Max);
        assert_eq!(rounds_in(&mut clock, 50), 50);
    }

    #[test]
    fn pause_toggles_and_keeps_the_speed() {
        let mut clock = Clock::new(Speed::Four);
        clock.toggle_pause();
        assert!(clock.is_paused());
        clock.faster();
        clock.toggle_pause();
        assert!(!clock.is_paused());
        assert_eq!(clock.speed(), Speed::Eight);
        assert_eq!(clock.until_next(), Some(Speed::Eight.pause()));
    }
}
