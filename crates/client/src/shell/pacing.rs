//! When the window draws its next frame. A frame is drawn only when something on screen
//! changes: right away while anything moves, at a low rate while only ambient animation
//! (animated water, a pulsing selection ring) plays, and otherwise not until input.
use std::time::Duration;
use web_time::Instant;

/// Time between frames of ambient animation alone (20 frames per second).
pub const AMBIENT_FRAME: Duration = Duration::from_millis(50);

/// The window's next frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRequest {
    /// Draw again as soon as the display allows.
    Now,
    /// Draw again at this time unless input comes first.
    At(Instant),
    /// Draw again only on input.
    Idle,
}

/// What on screen may change after the frame just drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Activity {
    /// The end of the turn runs: AI turns in frame slices, then their report.
    pub turn_running: bool,
    /// Presentation cues play (moves, attacks, marks, held figures).
    pub presentation: bool,
    /// The camera glides to a tile.
    pub glide: bool,
    /// The renderer finishes work over the next frames (terrain, chunk rebuilds).
    pub renderer_work: bool,
    /// Ambient animation is on screen and motion is not reduced.
    pub ambient: bool,
    /// The UI's own next repaint (egui): zero is now, `Duration::MAX` is never.
    pub ui_delay: Duration,
}

impl Default for Activity {
    fn default() -> Self {
        Self {
            turn_running: false,
            presentation: false,
            glide: false,
            renderer_work: false,
            ambient: false,
            ui_delay: Duration::MAX,
        }
    }
}

/// Ambient animation is on screen: the world is shown, motion is not reduced, and the
/// water is animated (quality preset) or the selection ring and route pulse.
pub fn ambient(world_shown: bool, reduced_motion: bool, water_animated: bool, pulse: bool) -> bool {
    world_shown && !reduced_motion && (water_animated || pulse)
}

/// The next frame after one drawn at `now`.
pub fn next_frame(activity: &Activity, now: Instant) -> FrameRequest {
    let moving = activity.turn_running || activity.presentation || activity.glide || activity.renderer_work;
    if moving || activity.ui_delay.is_zero() {
        return FrameRequest::Now;
    }
    let ui = now.checked_add(activity.ui_delay);
    let ambient = activity.ambient.then(|| now + AMBIENT_FRAME);
    match [ui, ambient].into_iter().flatten().min() {
        Some(at) => FrameRequest::At(at),
        None => FrameRequest::Idle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(activity: Activity) -> (FrameRequest, Instant) {
        let now = Instant::now();
        (next_frame(&activity, now), now)
    }

    #[test]
    fn a_still_map_waits_for_input() {
        assert_eq!(decide(Activity::default()).0, FrameRequest::Idle);
    }

    #[test]
    fn ambient_animation_alone_draws_twenty_frames_a_second() {
        let (request, now) = decide(Activity { ambient: true, ..Activity::default() });
        assert_eq!(request, FrameRequest::At(now + AMBIENT_FRAME));
    }

    #[test]
    fn reduced_motion_stops_ambient_frames() {
        assert!(ambient(true, false, true, false));
        assert!(ambient(true, false, false, true));
        assert!(!ambient(true, true, true, true));
        assert!(!ambient(true, false, false, false));
        assert!(!ambient(false, false, true, true));
    }

    #[test]
    fn a_camera_glide_draws_now() {
        assert_eq!(decide(Activity { glide: true, ..Activity::default() }).0, FrameRequest::Now);
    }

    #[test]
    fn ai_turns_draw_now() {
        let activity = Activity { turn_running: true, ambient: true, ..Activity::default() };
        assert_eq!(decide(activity).0, FrameRequest::Now);
    }

    #[test]
    fn playing_presentation_draws_now() {
        assert_eq!(decide(Activity { presentation: true, ..Activity::default() }).0, FrameRequest::Now);
    }

    #[test]
    fn terrain_being_built_draws_now() {
        assert_eq!(decide(Activity { renderer_work: true, ..Activity::default() }).0, FrameRequest::Now);
    }

    #[test]
    fn an_immediate_ui_repaint_draws_now() {
        assert_eq!(decide(Activity { ui_delay: Duration::ZERO, ..Activity::default() }).0, FrameRequest::Now);
    }

    #[test]
    fn a_delayed_ui_repaint_draws_at_its_time() {
        let delay = Duration::from_millis(500);
        let (request, now) = decide(Activity { ui_delay: delay, ..Activity::default() });
        assert_eq!(request, FrameRequest::At(now + delay));
    }

    #[test]
    fn the_earlier_of_ui_and_ambient_wins() {
        let soon = Duration::from_millis(30);
        let (request, now) = decide(Activity { ambient: true, ui_delay: soon, ..Activity::default() });
        assert_eq!(request, FrameRequest::At(now + soon));
        let late = Duration::from_secs(2);
        let (request, now) = decide(Activity { ambient: true, ui_delay: late, ..Activity::default() });
        assert_eq!(request, FrameRequest::At(now + AMBIENT_FRAME));
    }
}
