//! Touch gestures on the map, from raw finger events: tap = primary click, long press =
//! secondary click (orders), one-finger drag = pan, two fingers = pinch zoom and pan.
//!
//! Platform-neutral: a window loop (Android's, a browser's) feeds `Touches` the fingers that
//! egui did not take (a finger that starts on a panel belongs to egui) and applies the
//! `Gesture`s it returns to the shell (`App::pan_view`, `zoom_view`, `click`). The state
//! machine owns no clock: every call passes `now`, so tests run without waiting.
use std::time::Duration;

use web_time::Instant;

use super::module::MapButton;

/// A finger holding still this long makes a secondary click (the desktop's right button).
pub const LONG_PRESS: Duration = Duration::from_millis(450);

/// Where a finger stands (physical pixels) and how it got there.
#[derive(Debug, Clone, Copy)]
struct Finger {
    id: u64,
    start: (f32, f32),
    at: (f32, f32),
    since: Instant,
}

/// What the map should do after a finger event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// Drag the map by this many physical pixels.
    Pan { dx: f32, dy: f32 },
    /// Scale the view by `factor` (> 1 zooms in) around `at`.
    Zoom { factor: f32, at: (f32, f32) },
    /// A click at `at` with this button.
    Tap { at: (f32, f32), button: MapButton },
}

/// The fingers on the map. `slop` is the travel (physical pixels) below which a touch is
/// still a tap; it grows with the screen density.
#[derive(Debug, Clone)]
pub struct Touches {
    slop: f32,
    long_press: Duration,
    fingers: Vec<Finger>,
    /// The single finger has travelled beyond the slop: a drag, not a tap.
    panning: bool,
    /// The long press of the single finger already fired: its release is not a tap.
    long_pressed: bool,
    /// Two fingers were down at some point of this touch: nothing else happens until all lift.
    multi: bool,
}

/// How a finger event changes a touch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Started,
    Moved,
    Ended,
    Cancelled,
}

impl Touches {
    pub fn new(slop: f32) -> Self {
        Self {
            slop,
            long_press: LONG_PRESS,
            fingers: Vec::new(),
            panning: false,
            long_pressed: false,
            multi: false,
        }
    }

    /// The same, with another long-press time (a platform setting; slow test drivers).
    pub fn with_long_press(mut self, long_press: Duration) -> Self {
        self.long_press = long_press;
        self
    }

    /// A map touch is in progress (the map is being dragged or pressed).
    pub fn active(&self) -> bool {
        !self.fingers.is_empty()
    }

    /// The map moves under the finger or fingers: the renderer may draw coarser meanwhile.
    pub fn dragging(&self) -> bool {
        self.panning || self.multi
    }

    /// When `tick` next has something to do (the long press), for the loop's wake-up.
    pub fn deadline(&self) -> Option<Instant> {
        match (
            self.fingers.as_slice(),
            self.panning,
            self.long_pressed,
            self.multi,
        ) {
            ([finger], false, false, false) => Some(finger.since + self.long_press),
            _ => None,
        }
    }

    /// A finger event; `at` is in physical pixels. A finger that egui took (it started on a
    /// panel) is never passed in, and its later events find no finger and do nothing.
    pub fn event(&mut self, id: u64, phase: Phase, at: (f32, f32), now: Instant) -> Vec<Gesture> {
        match phase {
            Phase::Started => self.start(id, at, now),
            Phase::Moved => self.moved(id, at),
            Phase::Ended => {
                // The lift position counts as a last move (a driver may send no move in between).
                let changed = self.fingers.iter().any(|f| f.id == id && f.at != at);
                let mut out = if changed {
                    self.moved(id, at)
                } else {
                    Vec::new()
                };
                out.extend(self.end(id, at, false));
                out
            }
            Phase::Cancelled => self.end(id, at, true),
        }
    }

    fn start(&mut self, id: u64, at: (f32, f32), now: Instant) -> Vec<Gesture> {
        if self.fingers.iter().any(|f| f.id == id) {
            return Vec::new();
        }
        if self.fingers.len() >= 2 {
            return Vec::new();
        }
        self.fingers.push(Finger {
            id,
            start: at,
            at,
            since: now,
        });
        if self.fingers.len() == 2 {
            // A second finger ends any tap or drag in progress: from here on it is a pinch.
            self.multi = true;
            self.panning = false;
        }
        Vec::new()
    }

    fn moved(&mut self, id: u64, at: (f32, f32)) -> Vec<Gesture> {
        let Some(index) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        let before = self.fingers.clone();
        self.fingers[index].at = at;
        match (self.fingers.as_slice(), before.as_slice()) {
            ([now, other_now], [was, other_was]) if self.multi => {
                let (centre_was, centre_now) = (
                    midpoint(was.at, other_was.at),
                    midpoint(now.at, other_now.at),
                );
                let (gap_was, gap_now) = (
                    distance(was.at, other_was.at),
                    distance(now.at, other_now.at),
                );
                let mut out = Vec::new();
                if gap_was > 1.0 && gap_now > 1.0 && (gap_now - gap_was).abs() > f32::EPSILON {
                    out.push(Gesture::Zoom {
                        factor: gap_now / gap_was,
                        at: centre_now,
                    });
                }
                let (dx, dy) = (centre_now.0 - centre_was.0, centre_now.1 - centre_was.1);
                if dx != 0.0 || dy != 0.0 {
                    out.push(Gesture::Pan { dx, dy });
                }
                out
            }
            ([finger], _) if !self.multi => {
                if self.long_pressed {
                    return Vec::new();
                }
                if !self.panning {
                    if distance(finger.start, finger.at) < self.slop {
                        return Vec::new();
                    }
                    // Past the slop: the pan starts from where the finger went down, so the
                    // map does not lag behind the finger by the slop.
                    self.panning = true;
                    return vec![Gesture::Pan {
                        dx: finger.at.0 - finger.start.0,
                        dy: finger.at.1 - finger.start.1,
                    }];
                }
                let was = before[0].at;
                vec![Gesture::Pan {
                    dx: at.0 - was.0,
                    dy: at.1 - was.1,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn end(&mut self, id: u64, at: (f32, f32), cancelled: bool) -> Vec<Gesture> {
        let Some(index) = self.fingers.iter().position(|f| f.id == id) else {
            return Vec::new();
        };
        let finger = self.fingers.remove(index);
        let tap = !cancelled
            && self.fingers.is_empty()
            && !self.multi
            && !self.panning
            && !self.long_pressed
            && distance(finger.start, at) < self.slop;
        if self.fingers.is_empty() {
            *self = Self::new(self.slop).with_long_press(self.long_press);
        }
        match tap {
            true => vec![Gesture::Tap {
                at,
                button: MapButton::Primary,
            }],
            false => Vec::new(),
        }
    }

    /// The clock moved on: a single finger that rested long enough makes the secondary click.
    pub fn tick(&mut self, now: Instant) -> Vec<Gesture> {
        let [finger] = self.fingers.as_slice() else {
            return Vec::new();
        };
        if self.multi || self.panning || self.long_pressed || now < finger.since + self.long_press {
            return Vec::new();
        }
        self.long_pressed = true;
        vec![Gesture::Tap {
            at: finger.at,
            button: MapButton::Secondary,
        }]
    }
}

fn distance(a: (f32, f32), b: (f32, f32)) -> f32 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

fn midpoint(a: (f32, f32), b: (f32, f32)) -> (f32, f32) {
    ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(base: Instant, n: u64) -> Instant {
        base + Duration::from_millis(n)
    }

    #[test]
    fn a_short_touch_in_place_is_a_primary_click() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        assert!(touches
            .event(1, Phase::Started, (100.0, 200.0), t0)
            .is_empty());
        assert!(touches
            .event(1, Phase::Moved, (103.0, 201.0), ms(t0, 40))
            .is_empty());
        let out = touches.event(1, Phase::Ended, (103.0, 201.0), ms(t0, 80));
        assert_eq!(
            out,
            vec![Gesture::Tap {
                at: (103.0, 201.0),
                button: MapButton::Primary
            }]
        );
        assert!(!touches.active());
    }

    #[test]
    fn a_lift_far_from_the_last_move_still_pans_and_is_no_click() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (100.0, 100.0), t0);
        let out = touches.event(1, Phase::Ended, (300.0, 100.0), ms(t0, 50));
        assert_eq!(out, vec![Gesture::Pan { dx: 200.0, dy: 0.0 }]);
    }

    #[test]
    fn a_drag_pans_from_where_it_started_and_makes_no_click() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (100.0, 100.0), t0);
        let first = touches.event(1, Phase::Moved, (130.0, 100.0), ms(t0, 20));
        assert_eq!(first, vec![Gesture::Pan { dx: 30.0, dy: 0.0 }]);
        assert!(touches.dragging());
        let next = touches.event(1, Phase::Moved, (140.0, 95.0), ms(t0, 40));
        assert_eq!(next, vec![Gesture::Pan { dx: 10.0, dy: -5.0 }]);
        assert!(touches
            .event(1, Phase::Ended, (140.0, 95.0), ms(t0, 60))
            .is_empty());
        assert!(!touches.dragging());
    }

    #[test]
    fn a_long_press_is_a_secondary_click_once_and_its_release_is_silent() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (50.0, 60.0), t0);
        assert_eq!(touches.deadline(), Some(t0 + LONG_PRESS));
        assert!(touches.tick(ms(t0, 300)).is_empty());
        let out = touches.tick(ms(t0, 460));
        assert_eq!(
            out,
            vec![Gesture::Tap {
                at: (50.0, 60.0),
                button: MapButton::Secondary
            }]
        );
        assert!(touches.tick(ms(t0, 900)).is_empty(), "fires once");
        assert_eq!(touches.deadline(), None);
        assert!(touches
            .event(1, Phase::Ended, (50.0, 60.0), ms(t0, 1000))
            .is_empty());
    }

    #[test]
    fn moving_before_the_long_press_cancels_it() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (50.0, 60.0), t0);
        touches.event(1, Phase::Moved, (90.0, 60.0), ms(t0, 100));
        assert!(touches.tick(ms(t0, 600)).is_empty());
    }

    #[test]
    fn two_fingers_zoom_by_their_distance_and_pan_by_their_midpoint() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (100.0, 100.0), t0);
        touches.event(2, Phase::Started, (200.0, 100.0), ms(t0, 10));
        assert!(touches.dragging());
        // Spread to twice the distance, midpoint stays.
        let out = touches.event(2, Phase::Moved, (300.0, 100.0), ms(t0, 30));
        let zoom = out.iter().find_map(|g| match g {
            Gesture::Zoom { factor, at } => Some((*factor, *at)),
            _ => None,
        });
        assert_eq!(zoom, Some((2.0, (200.0, 100.0))));
        // The midpoint moved by 50 with the second finger: a pan too.
        assert!(out.contains(&Gesture::Pan { dx: 50.0, dy: 0.0 }));
        // Lifting one finger does not turn the other into a drag or a tap.
        assert!(touches
            .event(2, Phase::Ended, (300.0, 100.0), ms(t0, 50))
            .is_empty());
        assert!(touches
            .event(1, Phase::Moved, (160.0, 160.0), ms(t0, 60))
            .is_empty());
        assert!(touches
            .event(1, Phase::Ended, (160.0, 160.0), ms(t0, 70))
            .is_empty());
        assert!(!touches.active());
        assert!(!touches.dragging());
    }

    #[test]
    fn the_second_finger_cancels_a_pending_long_press() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (100.0, 100.0), t0);
        touches.event(2, Phase::Started, (200.0, 100.0), ms(t0, 100));
        assert_eq!(touches.deadline(), None);
        assert!(touches.tick(ms(t0, 700)).is_empty());
    }

    #[test]
    fn a_cancelled_touch_is_not_a_click() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        touches.event(1, Phase::Started, (10.0, 10.0), t0);
        assert!(touches
            .event(1, Phase::Cancelled, (10.0, 10.0), ms(t0, 30))
            .is_empty());
        assert!(!touches.active());
    }

    #[test]
    fn events_of_a_finger_egui_took_are_ignored() {
        let t0 = Instant::now();
        let mut touches = Touches::new(10.0);
        // Finger 7 started on a panel: the loop never reported it.
        assert!(touches.event(7, Phase::Moved, (5.0, 5.0), t0).is_empty());
        assert!(touches
            .event(7, Phase::Ended, (5.0, 5.0), ms(t0, 10))
            .is_empty());
        assert!(!touches.active());
    }
}
