//! The coordinate chain of ADR 0001 ("Coordinates: one chain and one grid"), as pure types and transforms.
//!
//! Symbols (the ADR's): `c` = the content rectangle's origin in the capture; `s` = viewer scale (capture pixels per
//! remote pixel; 1 for a local window or display); `r` = the target's scale factor (remote pixels per remote logical
//! point; for a local target the display's scale `h`); `h` = the host display's scale factor; `o` = the captured
//! window's origin on the host.
//!
//! ```text
//! logical --(x r)--> remote px --(x s, + c)--> capture px --(macOS: / h, + o   Windows: + o)--> host input
//! ```
//!
//! The tile grid and the change hashes live in the logical space, so tiles do not move when a window moves.

use crate::geometry::{CapturePx, GeometryError, HostInput, Logical, Map, Pt, Rect, RemotePx, Scale};

/// How the host takes input coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputSpace {
    /// macOS: global points (physical pixels divided by the display scale `h`); the window origin `o` is in points.
    MacPoints,
    /// Windows (Per-Monitor V2 DPI aware): physical virtual-screen pixels; the window origin `o` is in pixels.
    WindowsPixels,
}

/// Why a chain, a display or a layout was refused, or a box could not be placed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ChainError {
    /// A number was not finite or out of range.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
    /// A display has a zero or oversized pixel size.
    #[error("display {id} size {width} x {height} px must be between 1 and {max} px on each side")]
    DisplaySize {
        /// The display.
        id: u32,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Largest accepted side.
        max: u32,
    },
    /// A layout with no display.
    #[error("a display layout needs at least one display")]
    NoDisplays,
    /// A layout with more displays than supported.
    #[error("a display layout holds at most {max} displays, got {got}")]
    TooManyDisplays {
        /// Number given.
        got: usize,
        /// Limit.
        max: usize,
    },
    /// Two displays share an id.
    #[error("display id {0} is used twice")]
    DuplicateDisplay(u32),
    /// Two displays overlap in the host's input space.
    #[error("displays {0} and {1} overlap")]
    OverlappingDisplays(u32, u32),
    /// No display with this id.
    #[error("no display with id {0}")]
    UnknownDisplay(u32),
    /// A box lies on no display.
    #[error("the box lies outside every display")]
    OffScreen,
    /// A box sticks out of the only display it touches.
    #[error("the box leaves display {0}")]
    PartlyOffScreen(u32),
    /// A box spans two or more displays (a box never does).
    #[error("the box spans several displays")]
    SpansDisplays,
}

/// The five numbers that define a chain.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChainParams {
    /// `s`: capture pixels per remote pixel.
    pub viewer_scale: Scale,
    /// `r`: remote pixels per remote logical point.
    pub remote_scale: Scale,
    /// `h`: the host display's scale factor.
    pub host_scale: Scale,
    /// `c`: the content rectangle's origin in the capture.
    pub content_origin: Pt<CapturePx>,
    /// `o`: the captured window's origin in the host's input space.
    pub window_origin: Pt<HostInput>,
    /// Whether the host takes points or pixels.
    pub input: InputSpace,
}

/// One target's coordinate chain: the four maps between logical, remote, capture and host input coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Chain {
    params: ChainParams,
    logical_to_remote: Map<Logical, RemotePx>,
    remote_to_capture: Map<RemotePx, CapturePx>,
    capture_to_input: Map<CapturePx, HostInput>,
}

impl Chain {
    /// Builds the chain from its five numbers.
    pub fn new(params: ChainParams) -> Result<Self, ChainError> {
        let capture_scale = match params.input {
            InputSpace::MacPoints => Scale::new(1.0 / params.host_scale.x(), 1.0 / params.host_scale.y())?,
            InputSpace::WindowsPixels => Scale::uniform(1.0)?,
        };
        Ok(Self {
            params,
            logical_to_remote: Map::new(params.remote_scale, 0.0, 0.0)?,
            remote_to_capture: Map::new(params.viewer_scale, params.content_origin.x(), params.content_origin.y())?,
            capture_to_input: Map::new(capture_scale, params.window_origin.x(), params.window_origin.y())?,
        })
    }

    /// A local window or display: `s = 1`, `r = h`, and the capture pixel is a host physical pixel.
    pub fn local(
        host_scale: Scale,
        content_origin: Pt<CapturePx>,
        window_origin: Pt<HostInput>,
        input: InputSpace,
    ) -> Result<Self, ChainError> {
        Self::new(ChainParams {
            viewer_scale: Scale::uniform(1.0)?,
            remote_scale: host_scale,
            host_scale,
            content_origin,
            window_origin,
            input,
        })
    }

    /// The numbers this chain was built from.
    pub fn params(&self) -> &ChainParams {
        &self.params
    }

    /// Logical points to remote pixels (`x r`).
    pub fn logical_to_remote(&self) -> Map<Logical, RemotePx> {
        self.logical_to_remote
    }
    /// Remote pixels to capture pixels (`x s + c`).
    pub fn remote_to_capture(&self) -> Map<RemotePx, CapturePx> {
        self.remote_to_capture
    }
    /// Capture pixels to host input coordinates (macOS `/ h + o`, Windows `+ o`).
    pub fn capture_to_input(&self) -> Map<CapturePx, HostInput> {
        self.capture_to_input
    }
    /// The whole forward chain: logical points to host input coordinates (where a click lands).
    pub fn logical_to_input(&self) -> Map<Logical, HostInput> {
        self.logical_to_remote.then(&self.remote_to_capture).then(&self.capture_to_input)
    }
    /// Logical points to capture pixels (where a box lies in the frame).
    pub fn logical_to_capture(&self) -> Map<Logical, CapturePx> {
        self.logical_to_remote.then(&self.remote_to_capture)
    }
    /// The whole backward chain: host input coordinates to logical points.
    pub fn input_to_logical(&self) -> Map<HostInput, Logical> {
        self.logical_to_input().inverse()
    }
    /// Capture pixels to logical points (the grid the tiles use).
    pub fn capture_to_logical(&self) -> Map<CapturePx, Logical> {
        self.logical_to_capture().inverse()
    }
}

/// The nearest whole host pixel of an input point (for Windows `SendInput`); ties round away from zero.
pub fn nearest_pixel(p: Pt<HostInput>) -> (i64, i64) {
    (p.x().round() as i64, p.y().round() as i64)
}

/// Largest side of a display in pixels (16K).
pub const MAX_DISPLAY_SIDE: u32 = 16_384;
/// Most displays a layout holds.
pub const MAX_DISPLAYS: usize = 16;

/// One host display.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Display {
    id: u32,
    origin: Pt<HostInput>,
    width_px: u32,
    height_px: u32,
    scale: Scale,
}

impl Display {
    /// A display with its top-left corner in the host's input space (which may be negative), its pixel size and scale.
    pub fn new(id: u32, origin: Pt<HostInput>, width_px: u32, height_px: u32, scale: Scale) -> Result<Self, ChainError> {
        let ok = |v: u32| (1..=MAX_DISPLAY_SIDE).contains(&v);
        if !ok(width_px) || !ok(height_px) {
            return Err(ChainError::DisplaySize {
                id,
                width: width_px,
                height: height_px,
                max: MAX_DISPLAY_SIDE,
            });
        }
        Ok(Self {
            id,
            origin,
            width_px,
            height_px,
            scale,
        })
    }
    /// The display id.
    pub fn id(&self) -> u32 {
        self.id
    }
    /// The display's scale factor `h`.
    pub fn scale(&self) -> Scale {
        self.scale
    }
    /// Pixel size (width, height).
    pub fn size_px(&self) -> (u32, u32) {
        (self.width_px, self.height_px)
    }
    /// The display's area in the host's input space: points on macOS, pixels on Windows.
    pub fn extent(&self, input: InputSpace) -> Rect<HostInput> {
        let (w, h) = (f64::from(self.width_px), f64::from(self.height_px));
        let (w, h) = match input {
            InputSpace::MacPoints => (w / self.scale.x(), h / self.scale.y()),
            InputSpace::WindowsPixels => (w, h),
        };
        // Sizes are bounded by MAX_DISPLAY_SIDE and scales by MIN_SCALE, so this cannot leave the valid range.
        Rect::new(self.origin.x(), self.origin.y(), w, h).unwrap_or_else(|_| Rect::from_corners(self.origin, self.origin))
    }
}

/// The displays of one host, validated: unique ids, no overlap, a bounded count.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    input: InputSpace,
    displays: Vec<Display>,
}

impl Layout {
    /// Validates and builds a layout.
    pub fn new(input: InputSpace, displays: Vec<Display>) -> Result<Self, ChainError> {
        if displays.is_empty() {
            return Err(ChainError::NoDisplays);
        }
        if displays.len() > MAX_DISPLAYS {
            return Err(ChainError::TooManyDisplays {
                got: displays.len(),
                max: MAX_DISPLAYS,
            });
        }
        for (i, a) in displays.iter().enumerate() {
            for b in &displays[i + 1..] {
                if a.id == b.id {
                    return Err(ChainError::DuplicateDisplay(a.id));
                }
                if a.extent(input).intersection(&b.extent(input)).is_some() {
                    return Err(ChainError::OverlappingDisplays(a.id, b.id));
                }
            }
        }
        Ok(Self { input, displays })
    }
    /// The input space of this host.
    pub fn input(&self) -> InputSpace {
        self.input
    }
    /// The displays.
    pub fn displays(&self) -> &[Display] {
        &self.displays
    }
    /// The display with this id.
    pub fn display(&self, id: u32) -> Result<&Display, ChainError> {
        self.displays.iter().find(|d| d.id == id).ok_or(ChainError::UnknownDisplay(id))
    }
    /// The display holding a host point, if any. Displays are half-open, `[left, right) x [top, bottom)`: the first
    /// pixel of the next display, not the last of this one, owns the shared edge.
    pub fn display_at(&self, p: Pt<HostInput>) -> Option<&Display> {
        self.displays.iter().find(|d| {
            let e = d.extent(self.input);
            p.x() >= e.x() && p.x() < e.right() && p.y() >= e.y() && p.y() < e.bottom()
        })
    }
    /// The chain of a whole-display capture: `c = 0`, `s = 1`, `r = h`, `o` = the display's origin.
    pub fn chain_for_display(&self, id: u32) -> Result<Chain, ChainError> {
        let d = self.display(id)?;
        Chain::local(d.scale, Pt::new(0.0, 0.0)?, d.origin, self.input)
    }
    /// The display a box (in host input coordinates) lies on. A box never spans two displays.
    pub fn display_of_box(&self, b: &Rect<HostInput>) -> Result<&Display, ChainError> {
        // Whole-pixel rounding at a display edge may leave a sliver of this size.
        const EDGE_SLACK: f64 = 1e-6;
        let slack = Rect::new(
            b.x() + EDGE_SLACK,
            b.y() + EDGE_SLACK,
            (b.w() - 2.0 * EDGE_SLACK).max(0.0),
            (b.h() - 2.0 * EDGE_SLACK).max(0.0),
        )?;
        let mut touched = self
            .displays
            .iter()
            .filter(|d| slack.intersection(&d.extent(self.input)).is_some() || d.extent(self.input).contains(&slack));
        match (touched.next(), touched.next()) {
            (None, _) => Err(ChainError::OffScreen),
            (Some(_), Some(_)) => Err(ChainError::SpansDisplays),
            (Some(d), None) if d.extent(self.input).contains(&slack) => Ok(d),
            (Some(d), None) => Err(ChainError::PartlyOffScreen(d.id)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pt<S>(x: f64, y: f64) -> Pt<S> {
        Pt::new(x, y).unwrap()
    }
    fn sc(v: f64) -> Scale {
        Scale::uniform(v).unwrap()
    }
    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() <= 1e-6 * (1.0 + a.abs().max(b.abs()))
    }

    /// The ADR's viewer case: r = 1.5 with s = 0.75 on a 200 % macOS host, window at (100, 50) points, content at (0, 28).
    fn viewer(s: f64, r: f64, h: f64, input: InputSpace) -> Chain {
        Chain::new(ChainParams {
            viewer_scale: sc(s),
            remote_scale: sc(r),
            host_scale: sc(h),
            content_origin: pt(0.0, 28.0),
            window_origin: pt(100.0, 50.0),
            input,
        })
        .unwrap()
    }

    #[test]
    fn viewer_chain_with_fractional_remote_scale_and_shrunk_viewer() {
        let c = viewer(0.75, 1.5, 2.0, InputSpace::MacPoints);
        // Logical (200, 100) -> remote (300, 150) -> capture (225, 112.5 + 28) -> points (112.5, 70.25) + (100, 50).
        let capture = c.logical_to_capture().pt(pt(200.0, 100.0));
        assert!(near(capture.x(), 225.0) && near(capture.y(), 140.5));
        let input = c.logical_to_input().pt(pt(200.0, 100.0));
        assert!(near(input.x(), 212.5) && near(input.y(), 120.25));
        let back = c.input_to_logical().pt(input);
        assert!(near(back.x(), 200.0) && near(back.y(), 100.0));
    }

    #[test]
    fn windows_input_adds_the_origin_in_pixels_and_ignores_h() {
        let c = viewer(0.5, 1.5, 2.0, InputSpace::WindowsPixels);
        let input = c.logical_to_input().pt(pt(200.0, 100.0));
        assert!(near(input.x(), 100.0 + 150.0) && near(input.y(), 50.0 + 28.0 + 75.0));
        assert_eq!(nearest_pixel(pt(10.5, -10.5)), (11, -11));
        assert_eq!(nearest_pixel(pt(10.49, 0.2)), (10, 0));
    }

    #[test]
    fn local_chain_has_s_one_and_r_equal_h() {
        let c = Chain::local(sc(2.0), pt(0.0, 0.0), pt(0.0, 0.0), InputSpace::MacPoints).unwrap();
        assert_eq!((c.params().viewer_scale, c.params().remote_scale), (sc(1.0), sc(2.0)));
        // A logical point is one macOS point on a Retina display: the chain is the identity.
        let p = c.logical_to_input().pt(pt(321.0, 654.0));
        assert!(near(p.x(), 321.0) && near(p.y(), 654.0));
        // ... and the capture pixel is twice that.
        assert!(near(c.logical_to_capture().pt(pt(321.0, 654.0)).x(), 642.0));
        assert!(near(c.capture_to_logical().pt(pt(642.0, 0.0)).x(), 321.0));
        assert!(near(c.logical_to_remote().pt(pt(1.0, 1.0)).x(), 2.0));
        assert!(near(c.remote_to_capture().pt(pt(1.0, 1.0)).x(), 1.0));
        assert!(near(c.capture_to_input().pt(pt(2.0, 2.0)).x(), 1.0));
    }

    /// The ADR's three-display test layout, in Windows physical pixels.
    fn windows_layout() -> Layout {
        Layout::new(
            InputSpace::WindowsPixels,
            vec![
                Display::new(1, pt(0.0, 0.0), 1920, 1200, sc(1.0)).unwrap(),
                Display::new(2, pt(1920.0, 0.0), 3840, 2160, sc(2.0)).unwrap(),
                Display::new(3, pt(0.0, -1440.0), 2560, 1440, sc(1.5)).unwrap(),
            ],
        )
        .unwrap()
    }

    /// The same machine in macOS points: 100 %, 200 % and a non-integer 150 % panel (1706.67 x 960 points).
    fn mac_layout() -> Layout {
        Layout::new(
            InputSpace::MacPoints,
            vec![
                Display::new(1, pt(0.0, 0.0), 1920, 1200, sc(1.0)).unwrap(),
                Display::new(2, pt(1920.0, 0.0), 3840, 2160, sc(2.0)).unwrap(),
                Display::new(3, pt(0.0, -960.0), 2560, 1440, sc(1.5)).unwrap(),
            ],
        )
        .unwrap()
    }

    #[test]
    fn whole_display_chains_map_the_display_corners() {
        let w = windows_layout();
        // Display 2's last physical pixel is the host point (5759, 2159); logical = pixel / 2.
        let c = w.chain_for_display(2).unwrap();
        let p = c.logical_to_input().pt(pt(1919.5, 1079.5));
        assert!(near(p.x(), 1920.0 + 3839.0) && near(p.y(), 2159.0));
        // Display 3 has a negative origin and a 150 % scale.
        let c3 = w.chain_for_display(3).unwrap();
        let top_left = c3.logical_to_input().pt(pt(0.0, 0.0));
        assert!(near(top_left.x(), 0.0) && near(top_left.y(), -1440.0));
        let m = mac_layout();
        let c3 = m.chain_for_display(3).unwrap();
        let bottom_right = c3.logical_to_input().pt(pt(2560.0 / 1.5, 1440.0 / 1.5));
        assert!(near(bottom_right.x(), 2560.0 / 1.5) && near(bottom_right.y(), 0.0));
        assert_eq!(m.chain_for_display(9), Err(ChainError::UnknownDisplay(9)));
    }

    #[test]
    fn boxes_belong_to_one_display() {
        let w = windows_layout();
        let r = |x, y, ww, hh| Rect::<HostInput>::new(x, y, ww, hh).unwrap();
        assert_eq!(w.display_of_box(&r(10.0, 10.0, 50.0, 20.0)).unwrap().id(), 1);
        // Exactly at each edge of display 1 and of the negative-origin display 3.
        assert_eq!(w.display_of_box(&r(1900.0, 1180.0, 20.0, 20.0)).unwrap().id(), 1);
        assert_eq!(w.display_of_box(&r(0.0, -1440.0, 16.0, 16.0)).unwrap().id(), 3);
        assert_eq!(w.display_of_box(&r(2000.0, 100.0, 30.0, 30.0)).unwrap().id(), 2);
        assert_eq!(w.display_of_box(&r(1910.0, 10.0, 20.0, 20.0)), Err(ChainError::SpansDisplays));
        assert_eq!(w.display_of_box(&r(1910.0, 1190.0, 20.0, 20.0)), Err(ChainError::SpansDisplays));
        assert_eq!(w.display_of_box(&r(-9000.0, 0.0, 10.0, 10.0)), Err(ChainError::OffScreen));
        assert_eq!(w.display_of_box(&r(1910.0, 1190.0, 5.0, 20.0)), Err(ChainError::PartlyOffScreen(1)));
        assert_eq!(w.display_at(pt(2.0, 2.0)).map(Display::id), Some(1));
        assert!(w.display_at(pt(-5.0, -5.0)).is_none());
        // Half-open: x = 1920 is display 2's first pixel column, x = 1919 display 1's last; y = 0 belongs to displays 1 and
        // 2 (not to display 3 above, whose bottom edge is y = 0).
        assert_eq!(w.display_at(pt(1919.0, 5.0)).map(Display::id), Some(1));
        assert_eq!(w.display_at(pt(1920.0, 5.0)).map(Display::id), Some(2));
        assert_eq!(w.display_at(pt(5.0, 0.0)).map(Display::id), Some(1));
        assert_eq!(w.display_at(pt(5.0, -1.0)).map(Display::id), Some(3));
        assert!(w.display_at(pt(1920.0 + 3840.0, 5.0)).is_none());
    }

    #[test]
    fn layouts_are_validated() {
        let d = |id, x| Display::new(id, pt(x, 0.0), 100, 100, sc(1.0)).unwrap();
        assert_eq!(Layout::new(InputSpace::WindowsPixels, vec![]), Err(ChainError::NoDisplays));
        assert_eq!(
            Layout::new(InputSpace::WindowsPixels, vec![d(1, 0.0), d(1, 200.0)]),
            Err(ChainError::DuplicateDisplay(1))
        );
        assert_eq!(
            Layout::new(InputSpace::WindowsPixels, vec![d(1, 0.0), d(2, 50.0)]),
            Err(ChainError::OverlappingDisplays(1, 2))
        );
        assert!(Layout::new(InputSpace::WindowsPixels, vec![d(1, 0.0), d(2, 100.0)]).is_ok());
        let many = (0..=MAX_DISPLAYS as u32).map(|i| d(i, f64::from(i) * 100.0)).collect();
        assert!(matches!(
            Layout::new(InputSpace::WindowsPixels, many),
            Err(ChainError::TooManyDisplays { .. })
        ));
        assert!(matches!(
            Display::new(1, pt(0.0, 0.0), 0, 10, sc(1.0)),
            Err(ChainError::DisplaySize { .. })
        ));
        assert!(matches!(
            Display::new(1, pt(0.0, 0.0), MAX_DISPLAY_SIDE + 1, 10, sc(1.0)),
            Err(ChainError::DisplaySize { .. })
        ));
        let l = Layout::new(InputSpace::MacPoints, vec![d(1, 0.0)]).unwrap();
        assert_eq!(
            (l.input(), l.displays().len(), l.display(1).unwrap().size_px()),
            (InputSpace::MacPoints, 1, (100, 100))
        );
        assert_eq!(l.display(1).unwrap().scale(), sc(1.0));
    }

    #[test]
    fn a_shrunk_viewer_cannot_click_more_exactly_than_its_own_pixels() {
        // s = 0.25: one host pixel is four remote pixels, so rounding to a host pixel moves the click by up to two.
        let c = viewer(0.25, 1.0, 1.0, InputSpace::WindowsPixels);
        let to_input = c.remote_to_capture().then(&c.capture_to_input());
        let worst = (0..400)
            .map(|i| {
                let remote = pt::<RemotePx>(f64::from(i), 0.0);
                let (px, _) = nearest_pixel(to_input.pt(remote));
                (to_input.inverse().pt(pt::<HostInput>(px as f64, 0.0)).x() - remote.x()).abs()
            })
            .fold(0.0, f64::max);
        assert!(worst > 1.0 && worst <= 0.5 / 0.25 + 1e-9, "{worst}");
    }

    #[test]
    fn invalid_numbers_are_typed_errors() {
        assert!(matches!(Pt::<HostInput>::new(f64::NAN, 0.0), Err(GeometryError::NonFinite { .. })));
        assert!(matches!(
            ChainError::from(GeometryError::NonFinite {
                what: "x",
                value: f64::NAN
            }),
            ChainError::Geometry(_)
        ));
    }

    fn arb_scale_s() -> impl Strategy<Value = f64> {
        prop_oneof![Just(1.0), Just(0.75), Just(0.5), 0.25f64..2.0]
    }
    fn arb_scale_r() -> impl Strategy<Value = f64> {
        prop_oneof![Just(1.0), Just(1.5), Just(2.0), 0.5f64..4.0]
    }
    fn arb_scale_h() -> impl Strategy<Value = f64> {
        prop_oneof![Just(1.0), Just(1.25), Just(1.5), Just(2.0), Just(3.0), 1.0f64..3.5]
    }
    fn arb_chain() -> impl Strategy<Value = Chain> {
        (
            arb_scale_s(),
            arb_scale_r(),
            arb_scale_h(),
            -2000.0f64..2000.0,
            -2000.0f64..2000.0,
            0.0f64..200.0,
            0.0f64..200.0,
            any::<bool>(),
        )
            .prop_map(|(s, r, h, ox, oy, cx, cy, mac)| {
                Chain::new(ChainParams {
                    viewer_scale: sc(s),
                    remote_scale: sc(r),
                    host_scale: sc(h),
                    content_origin: pt(cx, cy),
                    window_origin: pt(ox, oy),
                    input: if mac { InputSpace::MacPoints } else { InputSpace::WindowsPixels },
                })
                .unwrap()
            })
    }

    proptest! {
        /// logical -> host input -> logical returns the same point (floating point, no rounding).
        #[test]
        fn forward_then_backward_is_identity(c in arb_chain(), x in -5000.0f64..5000.0, y in -5000.0f64..5000.0) {
            let p = pt::<Logical>(x, y);
            let back = c.input_to_logical().pt(c.logical_to_input().pt(p));
            prop_assert!(near(back.x(), x) && near(back.y(), y));
            let back = c.capture_to_logical().pt(c.logical_to_capture().pt(p));
            prop_assert!(near(back.x(), x) && near(back.y(), y));
        }

        /// A box maps to a box of the scaled size and back to the same box, so a box maps host -> remote -> host.
        #[test]
        fn boxes_round_trip(c in arb_chain(), x in -3000.0f64..3000.0, y in -3000.0f64..3000.0, w in 0.0f64..800.0, h in 0.0f64..800.0) {
            let b = Rect::<Logical>::new(x, y, w, h).unwrap();
            let input = c.logical_to_input().rect(b);
            let back = c.input_to_logical().rect(input);
            prop_assert!(near(back.x(), x) && near(back.y(), y) && near(back.w(), w) && near(back.h(), h));
            let k = c.logical_to_input().scale_x();
            prop_assert!(near(input.w(), w * k));
        }

        /// A click on a whole remote pixel, rounded to a whole host pixel (Windows), comes back within one remote pixel.
        /// That holds for a viewer scale `s >= 0.5`: half a host pixel is `0.5 / s` remote pixels, at most one. Below
        /// `s = 0.5` a host pixel spans more than two remote pixels and no click can be that exact (the error is `0.5 / s`,
        /// checked in `a_shrunk_viewer_cannot_click_more_exactly_than_its_own_pixels`).
        #[test]
        fn rounded_input_lands_within_one_pixel_of_the_finer_grid(
            s in prop_oneof![Just(0.5), Just(1.0), 0.5f64..2.0], r in arb_scale_r(), h in arb_scale_h(),
            ox in -2000i32..2000, oy in -2000i32..2000, rx in 0u32..4000, ry in 0u32..4000,
        ) {
            let c = Chain::new(ChainParams {
                viewer_scale: sc(s), remote_scale: sc(r), host_scale: sc(h),
                content_origin: pt(10.0, 30.0), window_origin: pt(f64::from(ox), f64::from(oy)),
                input: InputSpace::WindowsPixels,
            }).unwrap();
            let remote = pt::<RemotePx>(f64::from(rx), f64::from(ry));
            let to_input = c.remote_to_capture().then(&c.capture_to_input());
            let (px, py) = nearest_pixel(to_input.pt(remote));
            let back = to_input.inverse().pt(pt(px as f64, py as f64));
            let tolerance = 1.0 + 1e-6;
            prop_assert!((back.x() - remote.x()).abs() <= tolerance && (back.y() - remote.y()).abs() <= tolerance);
            // Host -> remote -> host: a whole host pixel returns exactly (floating point), hence within one pixel.
            let host = pt::<HostInput>(px as f64, py as f64);
            let again = to_input.pt(to_input.inverse().pt(host));
            prop_assert!((again.x() - host.x()).abs() <= 1.0 && (again.y() - host.y()).abs() <= 1.0);
        }

        /// Every box inside a generated multi-display layout belongs to exactly one display and a whole-display chain
        /// maps its logical box back inside that display.
        #[test]
        fn boxes_in_a_layout_have_one_display(lx in 0.0f64..1900.0, ly in 0.0f64..1100.0, w in 1.0f64..20.0, h in 1.0f64..20.0) {
            for layout in [windows_layout(), mac_layout()] {
                let chain = layout.chain_for_display(1).unwrap();
                let logical = Rect::<Logical>::new(lx, ly, w, h).unwrap();
                let host = chain.logical_to_input().rect(logical);
                let d = layout.display_of_box(&host).unwrap();
                prop_assert_eq!(d.id(), 1);
                let back = chain.input_to_logical().rect(host);
                prop_assert!(near(back.x(), lx) && near(back.y(), ly));
            }
        }
    }
}
