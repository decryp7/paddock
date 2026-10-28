//! Pixel geometry. All arithmetic is widened to `u64`, so a rectangle near `u32::MAX` never overflows.

/// A point in frame pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Point {
    /// Column.
    pub x: u32,
    /// Row.
    pub y: u32,
}

/// A rectangle in frame pixels (top-left origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

impl Rect {
    /// A rectangle from its corner and size.
    pub const fn new(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Exclusive right edge.
    pub fn right(&self) -> u64 {
        u64::from(self.x) + u64::from(self.w)
    }

    /// Exclusive bottom edge.
    pub fn bottom(&self) -> u64 {
        u64::from(self.y) + u64::from(self.h)
    }

    /// True when the rectangle covers no pixel.
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Area in pixels.
    pub fn area(&self) -> u64 {
        u64::from(self.w) * u64::from(self.h)
    }

    /// The click point: the centre, rounded down.
    pub fn center(&self) -> Point {
        Point {
            x: self.x.saturating_add(self.w / 2),
            y: self.y.saturating_add(self.h / 2),
        }
    }

    /// True when the pixel lies inside.
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.y >= self.y && u64::from(p.x) < self.right() && u64::from(p.y) < self.bottom()
    }

    /// The overlap of both rectangles; `None` when they do not overlap.
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x = self.x.max(o.x);
        let y = self.y.max(o.y);
        let r = self.right().min(o.right());
        let b = self.bottom().min(o.bottom());
        if r <= u64::from(x) || b <= u64::from(y) {
            return None;
        }
        // r and b are at most u32::MAX + u32::MAX, but they are bounded by min(), so the differences fit in u32.
        Some(Rect {
            x,
            y,
            w: u32::try_from(r - u64::from(x)).ok()?,
            h: u32::try_from(b - u64::from(y)).ok()?,
        })
    }

    /// Intersection over union in 0..=1; 0 for empty rectangles.
    pub fn iou(&self, o: &Rect) -> f32 {
        let inter = u128::from(self.intersect(o).map_or(0, |r| r.area()));
        let union = u128::from(self.area()) + u128::from(o.area()) - inter;
        if union == 0 { 0.0 } else { inter as f32 / union as f32 }
    }

    /// Clamps to a `width` x `height` frame; `None` when nothing is left.
    pub fn clamp_to(&self, width: u32, height: u32) -> Option<Rect> {
        self.intersect(&Rect::new(0, 0, width, height))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_contains_and_edges() {
        let r = Rect::new(10, 20, 5, 3);
        assert_eq!(r.center(), Point { x: 12, y: 21 });
        assert!(r.contains(Point { x: 10, y: 20 }));
        assert!(!r.contains(Point { x: 15, y: 20 }));
        assert!(!r.contains(Point { x: 9, y: 20 }));
        assert_eq!((r.right(), r.bottom(), r.area()), (15, 23, 15));
        assert!(Rect::new(0, 0, 0, 4).is_empty() && Rect::new(0, 0, 4, 0).is_empty() && !r.is_empty());
    }

    #[test]
    fn intersect_iou_and_clamp() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.intersect(&Rect::new(10, 0, 5, 5)), None);
        assert_eq!(a.intersect(&Rect::new(0, 10, 5, 5)), None);
        assert!((a.iou(&b) - 25.0 / 175.0).abs() < 1e-6);
        assert_eq!(a.iou(&a), 1.0);
        assert_eq!(Rect::new(0, 0, 0, 0).iou(&Rect::new(0, 0, 0, 0)), 0.0);
        assert_eq!(b.clamp_to(12, 12), Some(Rect::new(5, 5, 7, 7)));
        assert_eq!(b.clamp_to(5, 5), None);
    }

    #[test]
    fn extreme_rectangles_do_not_overflow() {
        let big = Rect::new(u32::MAX, u32::MAX, u32::MAX, u32::MAX);
        assert_eq!(big.right(), 2 * u64::from(u32::MAX));
        assert_eq!(big.clamp_to(10, 10), None);
        assert!(big.contains(Point { x: u32::MAX, y: u32::MAX }));
        assert_eq!(big.center().x, u32::MAX);
        let _ = big.iou(&Rect::new(0, 0, u32::MAX, u32::MAX));
        // The full-range overlap case: the width of the overlap still fits.
        assert_eq!(
            Rect::new(0, 0, u32::MAX, 1).intersect(&Rect::new(0, 0, u32::MAX, 1)),
            Some(Rect::new(0, 0, u32::MAX, 1))
        );
    }
}
