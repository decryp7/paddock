//! The oracle of the real-window checks (ADR 0043 §1 and §4): the inner height a dialog window must have, written as
//! the spec reads and independent of [`SizeState`](crate::size::SizeState), so a probe in a real window (the show probe
//! here, an app's own) and the state machine can disagree and show a bug. Pure; logical px.
//!
//! * `Height` kinds (decision, form, viewer): `max(floor, min(content, cap))`, where the cap is
//!   [`CAP_FRACTION`] of the work area less the frame; the floor beats the cap, the work area beats both
//!   (`work - frame`: the most a screen holds, and the window is clamped to it rather than cut off).
//! * `Manual` (tool): the declared height, capped the same way, never below the floor, never above the work area.

use crate::contract::{CAP_FRACTION, SizeMode};

/// What a screen leaves a window: its work area's height and the visible frame's (title bar and borders).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Screen {
    pub work_height: f64,
    pub frame_height: f64,
}

impl Screen {
    /// The most inner height the screen holds.
    pub fn avail(&self) -> f64 {
        (self.work_height - self.frame_height).max(1.0)
    }

    /// The most a window of its preferred size may be (the cap).
    pub fn cap(&self) -> f64 {
        self.work_height * CAP_FRACTION - self.frame_height
    }
}

/// The inner height a freshly opened dialog window must have. `content` is the page's measured content height,
/// `declared` the contract's height for a `Manual` kind (`None`: `floor`), `floor` the kind's minimum height.
pub fn opening_height(
    mode: SizeMode,
    floor: f64,
    content: f64,
    declared: Option<f64>,
    screen: Screen,
) -> f64 {
    let wanted = match mode {
        SizeMode::Height { .. } => content.ceil(),
        SizeMode::Manual => declared.unwrap_or(floor),
    };
    wanted.min(screen.cap()).max(floor).min(screen.avail())
}

/// The accepted range of the same height where the frame is not known exactly: Windows' outer minus inner counts the
/// invisible resize borders (about 8 px a side), so the visible frame lies between `full - invisible` and `full`.
pub fn opening_range(
    mode: SizeMode,
    floor: f64,
    content: f64,
    declared: Option<f64>,
    work_height: f64,
    full_frame: f64,
    invisible: f64,
) -> (f64, f64) {
    let at = |frame: f64| {
        opening_height(
            mode,
            floor,
            content,
            declared,
            Screen {
                work_height,
                frame_height: frame,
            },
        )
    };
    let (a, b) = (at(full_frame), at((full_frame - invisible).max(0.0)));
    (a.min(b), a.max(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{DIALOG_MIN, Kind, VIEWER_MIN};

    fn mode(kind: Kind) -> SizeMode {
        kind.policy().size_mode
    }

    fn screen(work: f64) -> Screen {
        Screen {
            work_height: work,
            frame_height: 31.0,
        }
    }

    #[test]
    fn a_short_decision_opens_at_the_floor_and_a_tall_one_at_its_content_up_to_the_cap() {
        let m = mode(Kind::Decision);
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 148.0, None, screen(720.0)),
            200.0
        );
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 300.2, None, screen(720.0)),
            301.0
        );
        // 75 % of 720 less a 31 px frame is 509.
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 900.0, None, screen(720.0)),
            509.0
        );
    }

    #[test]
    fn a_viewer_never_opens_below_its_floor() {
        let m = mode(Kind::Viewer);
        assert_eq!(
            opening_height(m, VIEWER_MIN.height, 174.0, None, screen(1000.0)),
            560.0
        );
        assert_eq!(
            opening_height(m, VIEWER_MIN.height, 640.0, None, screen(1000.0)),
            640.0
        );
    }

    #[test]
    fn on_a_small_screen_the_floor_gives_way_to_the_work_area_and_nothing_is_cut_off() {
        // The Windows VM at 150 %: a 696 px work area is 464 logical px.
        let small = screen(464.0);
        assert_eq!(small.avail(), 433.0);
        assert_eq!(
            opening_height(mode(Kind::Viewer), VIEWER_MIN.height, 174.0, None, small),
            433.0
        );
        // A decision's floor (200) still fits; its content cap is 75 % less the frame, below its floor: the floor wins.
        assert_eq!(
            opening_height(mode(Kind::Decision), DIALOG_MIN.height, 174.0, None, small),
            200.0
        );
        // On a screen smaller than the floor the work area wins.
        assert_eq!(
            opening_height(
                mode(Kind::Decision),
                DIALOG_MIN.height,
                174.0,
                None,
                screen(200.0)
            ),
            169.0
        );
    }

    #[test]
    fn a_tool_opens_at_its_declared_height_capped_by_the_screen() {
        let m = mode(Kind::Tool);
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 0.0, Some(620.0), screen(1080.0)),
            620.0
        );
        // 75 % of the VM's 720 px work area less the frame: 509; the run measured 540 with a 0 px frame in the cap.
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 0.0, Some(620.0), screen(720.0)),
            509.0
        );
        assert_eq!(
            opening_height(m, DIALOG_MIN.height, 0.0, None, screen(1080.0)),
            200.0
        );
    }

    #[test]
    fn the_range_covers_a_frame_that_counts_the_invisible_borders() {
        // Outer minus inner is 39 with about 8 invisible px a side: the visible frame is 31 to 39.
        let (lo, hi) = opening_range(
            mode(Kind::Viewer),
            VIEWER_MIN.height,
            174.0,
            None,
            464.0,
            39.0,
            16.0,
        );
        assert_eq!((lo, hi), (425.0, 441.0));
        assert!((lo..=hi).contains(&433.0));
    }
}
