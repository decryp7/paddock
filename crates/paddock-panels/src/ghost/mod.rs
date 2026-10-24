//! The drag ghost: while a panel's tab is dragged outside its window, a small native window follows the pointer over
//! other apps and displays and shows where the popped-out window will open (its title bar with the panel's icon and
//! name, and a dimmed snapshot of the panel).
//!
//! The ghost is one hidden window (`drag-ghost`), created on the first drag and reused: undecorated, always on top, not
//! in the task bar, never focused, click-through and transparent where the platform allows. It loads a static page
//! ([`page`]) that has no commands and no capabilities; the shell hands it what to show.
//!
//! A drag session belongs to the window that started it: `drag_ghost_start` (content and size), then `drag_ghost_show` as
//! the pointer leaves or re-enters the window, and `drag_ghost_end`, which hides the ghost and answers where it was, so
//! the pop-out opens exactly there. While a session is active a thread follows the global cursor at about 60 Hz. The
//! ghost is never left behind: it hides when the session ends, when the primary button is released without an end (a lost
//! release), when its window loses focus with the button up, when the window is destroyed, and after a time limit.
//!
//! This module is the pure part (the session state machine, geometry, request parsing); the window work is in the Tauri
//! adapter. Wayland does not let an app place its windows, so there `drag_ghost_start` answers `false` and the UI keeps
//! its in-window preview.

pub mod geometry;
pub mod page;
pub mod request;

pub use request::GhostContent;

use geometry::{GhostSize, Placement, Screen, place};
use std::time::Duration;

/// The ghost window's label (no capability pattern matches it, and no command accepts it).
pub const GHOST_LABEL: &str = "drag-ghost";
/// The page it shows, served by the app from its public folder ([`page`]).
pub const GHOST_PAGE: &str = "drag-ghost.html";
/// How often the ghost follows the cursor.
pub const TICK: Duration = Duration::from_millis(16);
/// After this many ticks with the primary button up and no end from the UI, the drag is over: hide.
pub const RELEASED_TICKS: u32 = 15;
/// No drag lasts longer: the ghost hides and the session ends.
pub const MAX_DRAG: Duration = Duration::from_secs(120);

/// The drag session and the ghost's content.
#[derive(Default)]
pub struct State {
    session: Option<Session>,
    /// What the page shows (kept to hand it over again when the page reloads).
    content: Option<GhostContent>,
    /// The page has loaded and can take content.
    loaded: bool,
    next: u64,
}

/// One drag, owned by the window that started it.
#[derive(Debug, Clone)]
pub struct Session {
    id: u64,
    host: String,
    size: GhostSize,
    screens: Vec<Screen>,
    visible: bool,
    /// Hidden because the button was released without an end: the end still reports where the ghost was.
    released: bool,
    /// Where the ghost was put last.
    last: Option<Placement>,
}

/// What the tracker does on one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tick {
    /// Nothing to do this time.
    Idle,
    /// Move the ghost here.
    Move(Placement),
    /// Hide the ghost and stop following.
    Hide,
    /// The session is gone or was replaced: stop following.
    Stop,
}

/// What `drag_ghost_show` does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Visibility {
    Show(Placement),
    Hide,
    /// Not this window's session, or nowhere to show it.
    Ignore,
}

impl State {
    /// A session is running.
    pub fn active(&self) -> bool {
        self.session.is_some()
    }

    /// The content the page shows now.
    pub fn content(&self) -> Option<&GhostContent> {
        self.content.as_ref()
    }

    /// The page has loaded and can take content.
    pub fn loaded(&self) -> bool {
        self.loaded
    }

    /// The page loaded (or went away).
    pub fn set_loaded(&mut self, loaded: bool) {
        self.loaded = loaded;
    }

    /// The ghost window is gone: the page state and the session reset.
    pub fn reset(&mut self) {
        self.loaded = false;
        self.session = None;
    }

    /// A new session for `host` (replacing any other); its id, and whether the page can take the content now.
    pub fn begin(
        &mut self,
        host: &str,
        content: GhostContent,
        size: GhostSize,
        screens: Vec<Screen>,
    ) -> (u64, bool) {
        self.next += 1;
        self.session = Some(Session {
            id: self.next,
            host: host.to_string(),
            size,
            screens,
            visible: false,
            released: false,
            last: None,
        });
        self.content = Some(content);
        (self.next, self.loaded)
    }

    pub fn session_of(&mut self, host: &str) -> Option<&mut Session> {
        self.session.as_mut().filter(|s| s.host == host)
    }

    pub fn set_visible(
        &mut self,
        host: &str,
        visible: bool,
        cursor: Option<(f64, f64)>,
    ) -> Visibility {
        let Some(s) = self.session_of(host) else {
            return Visibility::Ignore;
        };
        if !visible {
            s.visible = false;
            return Visibility::Hide;
        }
        match cursor.and_then(|c| place(c, s.size, &s.screens)).or(s.last) {
            Some(p) => {
                s.visible = true;
                s.released = false;
                s.last = Some(p);
                Visibility::Show(p)
            }
            None => Visibility::Ignore,
        }
    }

    /// Ends `host`'s session: where the ghost was if it was showing (or was hidden by a lost release).
    pub fn finish(&mut self, host: &str) -> Option<Placement> {
        let s = self.session.take_if(|s| s.host == host)?;
        if s.visible || s.released {
            s.last
        } else {
            None
        }
    }

    /// One step of the tracker of session `id`.
    pub fn tick(
        &mut self,
        id: u64,
        cursor: Option<(f64, f64)>,
        released: bool,
        expired: bool,
    ) -> Tick {
        let Some(s) = self.session.as_mut().filter(|s| s.id == id) else {
            return Tick::Stop;
        };
        if expired {
            self.session = None;
            return Tick::Hide;
        }
        if released {
            s.released = s.visible;
            s.visible = false;
            return Tick::Hide;
        }
        if !s.visible {
            return Tick::Idle;
        }
        match cursor.and_then(|c| place(c, s.size, &s.screens)) {
            Some(p) if s.last != Some(p) => {
                s.last = Some(p);
                Tick::Move(p)
            }
            _ => Tick::Idle,
        }
    }
}

/// Can windows be placed on screen here? Not on Wayland (unless GTK runs on XWayland), where the compositor
/// decides and ignores the position an app asks for.
pub fn positioning_supported(
    os: &str,
    wayland_display: Option<&str>,
    gdk_backend: Option<&str>,
) -> bool {
    if os != "linux" {
        return true;
    }
    let forced_x11 = gdk_backend.is_some_and(|b| {
        b.split(',')
            .next()
            .is_some_and(|first| first.trim() == "x11")
    });
    forced_x11 || wayland_display.is_none_or(str::is_empty)
}

/// The script that hands the page its content: a JSON literal, never markup.
pub fn content_script(content: &GhostContent) -> Option<String> {
    let json = serde_json::to_string(content).ok()?;
    Some(format!(
        "window.paddockGhost && window.paddockGhost({json});"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use geometry::PhysRect;

    const MAIN: &str = "main";

    const SIZE: GhostSize = GhostSize {
        width: 400.0,
        height: 300.0,
        grab_x: 100.0,
        grab_y: 10.0,
    };

    fn content() -> GhostContent {
        GhostContent {
            title: "Inspector".into(),
            icon: None,
            snapshot: None,
            dark: false,
        }
    }

    fn screens() -> Vec<Screen> {
        vec![Screen {
            bounds: PhysRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            work: PhysRect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1040,
            },
            scale: 1.0,
        }]
    }

    fn started() -> (State, u64) {
        let mut s = State::default();
        let (id, loaded) = s.begin(MAIN, content(), SIZE, screens());
        assert!(!loaded);
        (s, id)
    }

    #[test]
    fn the_ghost_follows_the_cursor_only_while_shown() {
        let (mut s, id) = started();
        assert_eq!(
            s.tick(id, Some((500.0, 500.0)), false, false),
            Tick::Idle,
            "hidden: not moved"
        );
        let shown = Placement {
            rect: PhysRect {
                x: 400,
                y: 490,
                width: 400,
                height: 300,
            },
            scale: 1.0,
        };
        assert_eq!(
            s.set_visible(MAIN, true, Some((500.0, 500.0))),
            Visibility::Show(shown)
        );
        assert_eq!(
            s.tick(id, Some((500.0, 500.0)), false, false),
            Tick::Idle,
            "unchanged: not moved"
        );
        let moved = Placement {
            rect: PhysRect {
                x: 700,
                y: 590,
                width: 400,
                height: 300,
            },
            scale: 1.0,
        };
        assert_eq!(
            s.tick(id, Some((800.0, 600.0)), false, false),
            Tick::Move(moved)
        );
        assert_eq!(
            s.tick(id, None, false, false),
            Tick::Idle,
            "no cursor: stays"
        );
        assert_eq!(s.set_visible(MAIN, false, None), Visibility::Hide);
        assert_eq!(s.tick(id, Some((10.0, 10.0)), false, false), Tick::Idle);
    }

    #[test]
    fn the_end_reports_where_the_ghost_was_only_when_it_was_showing() {
        let (mut s, _) = started();
        s.set_visible(MAIN, true, Some((500.0, 500.0)));
        let b = s.finish(MAIN).unwrap().logical();
        assert_eq!((b.x, b.y, b.width, b.height), (400.0, 490.0, 400.0, 300.0));
        assert!(s.session.is_none());
        assert_eq!(s.finish(MAIN), None, "ended once");
        let (mut s, _) = started();
        s.set_visible(MAIN, true, Some((500.0, 500.0)));
        s.set_visible(MAIN, false, None);
        assert_eq!(
            s.finish(MAIN),
            None,
            "back over the window: no pop-out position"
        );
    }

    #[test]
    fn a_session_belongs_to_its_window() {
        let (mut s, id) = started();
        assert_eq!(
            s.set_visible("w-2", true, Some((1.0, 1.0))),
            Visibility::Ignore
        );
        assert_eq!(s.finish("w-2"), None);
        assert!(s.session.is_some(), "another window cannot end it");
        // A new drag replaces the old one, whose tracker stops.
        let (next, _) = s.begin("w-2", content(), SIZE, screens());
        assert_eq!(s.tick(id, Some((1.0, 1.0)), false, false), Tick::Stop);
        assert_ne!(s.tick(next, Some((1.0, 1.0)), false, false), Tick::Stop);
    }

    #[test]
    fn a_lost_release_hides_the_ghost_and_the_end_still_says_where() {
        let (mut s, id) = started();
        s.set_visible(MAIN, true, Some((500.0, 500.0)));
        assert_eq!(s.tick(id, Some((500.0, 500.0)), true, false), Tick::Hide);
        assert!(s.finish(MAIN).is_some());
        // Released while hidden: nothing to report.
        let (mut s, id) = started();
        assert_eq!(s.tick(id, None, true, false), Tick::Hide);
        assert_eq!(s.finish(MAIN), None);
    }

    #[test]
    fn a_drag_that_never_ends_is_given_up() {
        let (mut s, id) = started();
        s.set_visible(MAIN, true, Some((500.0, 500.0)));
        assert_eq!(s.tick(id, Some((500.0, 500.0)), false, true), Tick::Hide);
        assert!(s.session.is_none());
        assert_eq!(s.tick(id, None, false, false), Tick::Stop);
    }

    #[test]
    fn without_a_cursor_or_monitors_nothing_is_shown() {
        let mut s = State::default();
        s.begin(MAIN, content(), SIZE, Vec::new());
        assert_eq!(
            s.set_visible(MAIN, true, Some((1.0, 1.0))),
            Visibility::Ignore
        );
        let (mut s, _) = started();
        assert_eq!(s.set_visible(MAIN, true, None), Visibility::Ignore);
        // Shown before, the last place is kept when the cursor cannot be read.
        s.set_visible(MAIN, true, Some((500.0, 500.0)));
        assert!(matches!(
            s.set_visible(MAIN, true, None),
            Visibility::Show(_)
        ));
    }

    #[test]
    fn wayland_cannot_place_windows() {
        assert!(positioning_supported("windows", None, None));
        assert!(positioning_supported("macos", Some("wayland-0"), None));
        assert!(positioning_supported("linux", None, None), "X11");
        assert!(positioning_supported("linux", Some(""), None));
        assert!(!positioning_supported("linux", Some("wayland-0"), None));
        assert!(!positioning_supported(
            "linux",
            Some("wayland-0"),
            Some("wayland,x11")
        ));
        assert!(
            positioning_supported("linux", Some("wayland-0"), Some("x11")),
            "XWayland"
        );
        assert!(positioning_supported(
            "linux",
            Some("wayland-0"),
            Some(" x11 ,wayland")
        ));
    }

    #[test]
    fn the_page_gets_json_never_markup() {
        let script = content_script(&GhostContent {
            title: "</script><img src=x onerror=alert(1)>\"".into(),
            icon: Some("M1 1".into()),
            snapshot: None,
            dark: true,
        })
        .unwrap();
        assert!(script.starts_with("window.paddockGhost && window.paddockGhost({"));
        let json = script
            .trim_start_matches("window.paddockGhost && window.paddockGhost(")
            .trim_end_matches(");");
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        assert_eq!(v["title"], "</script><img src=x onerror=alert(1)>\"");
        assert_eq!(v["snapshot"], serde_json::Value::Null);
    }
}
