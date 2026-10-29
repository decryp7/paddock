//! Typed points, rectangles and affine maps. A coordinate space is a zero-sized marker type, so a value in one space
//! cannot be used as another by accident; the only way across is a [`Map`].

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::marker::PhantomData;

/// Largest absolute coordinate accepted. Bounds every input, so no transform in this crate can overflow.
pub const MAX_COORD: f64 = 1.0e7;
/// Smallest accepted scale factor.
pub const MIN_SCALE: f64 = 1.0 / 64.0;
/// Largest accepted scale factor.
pub const MAX_SCALE: f64 = 64.0;

/// Why a geometric value was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum GeometryError {
    /// A coordinate was NaN or infinite.
    #[error("{what} must be a finite number, got {value}")]
    NonFinite {
        /// Name of the offending field.
        what: &'static str,
        /// The value.
        value: f64,
    },
    /// A coordinate or size was outside the supported range.
    #[error("{what} = {value} is outside the supported range {min} to {max}")]
    OutOfRange {
        /// Name of the offending field.
        what: &'static str,
        /// The value.
        value: f64,
        /// Lowest accepted value.
        min: f64,
        /// Highest accepted value.
        max: f64,
    },
}

fn check(what: &'static str, value: f64, min: f64, max: f64) -> Result<f64, GeometryError> {
    if !value.is_finite() {
        return Err(GeometryError::NonFinite { what, value });
    }
    if value < min || value > max {
        return Err(GeometryError::OutOfRange { what, value, min, max });
    }
    Ok(value)
}

/// Marker: remote logical points (the 100 % scale the model was trained at). The tile grid lives here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Logical {}
/// Marker: remote pixels (logical points times the target's scale factor `r`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RemotePx {}
/// Marker: capture pixels (the frame the capture API delivers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CapturePx {}
/// Marker: the host's input space: global points on macOS, physical virtual-screen pixels on Windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HostInput {}

/// A point in coordinate space `S`.
pub struct Pt<S> {
    x: f64,
    y: f64,
    space: PhantomData<S>,
}

impl<S> Pt<S> {
    /// The point (0, 0).
    pub fn from_origin() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            space: PhantomData,
        }
    }
    /// Builds a point; both coordinates must be finite and within [`MAX_COORD`].
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            x: check("x", x, -MAX_COORD, MAX_COORD)?,
            y: check("y", y, -MAX_COORD, MAX_COORD)?,
            space: PhantomData,
        })
    }
    /// Horizontal coordinate.
    pub fn x(&self) -> f64 {
        self.x
    }
    /// Vertical coordinate.
    pub fn y(&self) -> f64 {
        self.y
    }
}

/// An axis-aligned rectangle in coordinate space `S`, with a non-negative width and height.
pub struct Rect<S> {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    space: PhantomData<S>,
}

macro_rules! plain_traits {
    ($t:ident { $($f:ident),+ }) => {
        impl<S> Clone for $t<S> { fn clone(&self) -> Self { *self } }
        impl<S> Copy for $t<S> {}
        impl<S> PartialEq for $t<S> { fn eq(&self, o: &Self) -> bool { $(self.$f == o.$f)&&+ } }
        impl<S> fmt::Debug for $t<S> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($t))$(.field(stringify!($f), &self.$f))+.finish()
            }
        }
    };
}
plain_traits!(Pt { x, y });
plain_traits!(Rect { x, y, w, h });

impl<S> Rect<S> {
    /// Builds a rectangle from its origin and size; the size must be non-negative and every value finite and bounded.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            x: check("x", x, -MAX_COORD, MAX_COORD)?,
            y: check("y", y, -MAX_COORD, MAX_COORD)?,
            w: check("width", w, 0.0, MAX_COORD)?,
            h: check("height", h, 0.0, MAX_COORD)?,
            space: PhantomData,
        })
    }
    /// Builds a rectangle from two opposite corners in any order.
    pub fn from_corners(a: Pt<S>, b: Pt<S>) -> Self {
        let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        Self {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
            space: PhantomData,
        }
    }
    /// Left edge.
    pub fn x(&self) -> f64 {
        self.x
    }
    /// Top edge.
    pub fn y(&self) -> f64 {
        self.y
    }
    /// Width.
    pub fn w(&self) -> f64 {
        self.w
    }
    /// Height.
    pub fn h(&self) -> f64 {
        self.h
    }
    /// Right edge.
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    /// Bottom edge.
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    /// Area (zero for a degenerate rectangle).
    pub fn area(&self) -> f64 {
        self.w * self.h
    }
    /// The centre point.
    pub fn center(&self) -> Pt<S> {
        Pt {
            x: self.x + self.w / 2.0,
            y: self.y + self.h / 2.0,
            space: PhantomData,
        }
    }
    /// True when the point lies inside or on the edge.
    pub fn contains_pt(&self, p: Pt<S>) -> bool {
        p.x >= self.x && p.x <= self.right() && p.y >= self.y && p.y <= self.bottom()
    }
    /// True when `other` lies wholly inside this rectangle (edges included).
    pub fn contains(&self, other: &Rect<S>) -> bool {
        other.x >= self.x && other.y >= self.y && other.right() <= self.right() && other.bottom() <= self.bottom()
    }
    /// The overlapping rectangle, or `None` when the interiors do not overlap (touching edges give `None`).
    pub fn intersection(&self, other: &Rect<S>) -> Option<Rect<S>> {
        let (x0, y0) = (self.x.max(other.x), self.y.max(other.y));
        let (x1, y1) = (self.right().min(other.right()), self.bottom().min(other.bottom()));
        (x1 > x0 && y1 > y0).then_some(Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
            space: PhantomData,
        })
    }
    /// True when the rectangles overlap or touch, allowing `tolerance` units of gap on each axis.
    pub fn touches(&self, other: &Rect<S>, tolerance: f64) -> bool {
        let t = tolerance.max(0.0);
        self.x <= other.right() + t && other.x <= self.right() + t && self.y <= other.bottom() + t && other.y <= self.bottom() + t
    }
    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect<S>) -> Rect<S> {
        let (x0, y0) = (self.x.min(other.x), self.y.min(other.y));
        let (x1, y1) = (self.right().max(other.right()), self.bottom().max(other.bottom()));
        Rect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
            space: PhantomData,
        }
    }
    /// Intersection over union in `[0, 1]`; zero when either rectangle is empty or they do not overlap.
    pub fn iou(&self, other: &Rect<S>) -> f64 {
        let inter = self.intersection(other).map_or(0.0, |r| r.area());
        let union = self.area() + other.area() - inter;
        if union > 0.0 { inter / union } else { 0.0 }
    }
    /// The part of this rectangle inside `bounds`, or `None` when nothing is left.
    pub fn clip(&self, bounds: &Rect<S>) -> Option<Rect<S>> {
        self.intersection(bounds)
    }
}

impl<S> Serialize for Pt<S> {
    fn serialize<T: Serializer>(&self, s: T) -> Result<T::Ok, T::Error> {
        [self.x, self.y].serialize(s)
    }
}
impl<'de, S> Deserialize<'de> for Pt<S> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [x, y] = <[f64; 2]>::deserialize(d)?;
        Pt::new(x, y).map_err(D::Error::custom)
    }
}
impl<S> Serialize for Rect<S> {
    fn serialize<T: Serializer>(&self, s: T) -> Result<T::Ok, T::Error> {
        [self.x, self.y, self.w, self.h].serialize(s)
    }
}
impl<'de, S> Deserialize<'de> for Rect<S> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let [x, y, w, h] = <[f64; 4]>::deserialize(d)?;
        Rect::new(x, y, w, h).map_err(D::Error::custom)
    }
}

/// A positive per-axis scale factor within [`MIN_SCALE`] and [`MAX_SCALE`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    x: f64,
    y: f64,
}

impl Scale {
    /// The factor 1.
    pub fn unit() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
    /// The same factor on both axes.
    pub fn uniform(v: f64) -> Result<Self, GeometryError> {
        Self::new(v, v)
    }
    /// Separate factors per axis (a viewer window can scale width and height slightly differently).
    pub fn new(x: f64, y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            x: check("scale x", x, MIN_SCALE, MAX_SCALE)?,
            y: check("scale y", y, MIN_SCALE, MAX_SCALE)?,
        })
    }
    /// Horizontal factor.
    pub fn x(&self) -> f64 {
        self.x
    }
    /// Vertical factor.
    pub fn y(&self) -> f64 {
        self.y
    }
}

/// A scale-then-translate map from space `A` to space `B`: `out = in * scale + offset`, per axis. Because scales are
/// positive, rectangles map to rectangles.
pub struct Map<A, B> {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
    spaces: PhantomData<(A, B)>,
}

impl<A, B> Clone for Map<A, B> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<A, B> Copy for Map<A, B> {}
impl<A, B> PartialEq for Map<A, B> {
    fn eq(&self, o: &Self) -> bool {
        self.sx == o.sx && self.sy == o.sy && self.tx == o.tx && self.ty == o.ty
    }
}
impl<A, B> fmt::Debug for Map<A, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Map")
            .field("sx", &self.sx)
            .field("sy", &self.sy)
            .field("tx", &self.tx)
            .field("ty", &self.ty)
            .finish()
    }
}

impl<A, B> Map<A, B> {
    /// The identity map.
    pub fn identity() -> Self {
        Self {
            sx: 1.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
            spaces: PhantomData,
        }
    }
    /// `out = in * scale + offset`.
    pub fn new(scale: Scale, offset_x: f64, offset_y: f64) -> Result<Self, GeometryError> {
        Ok(Self {
            sx: scale.x,
            sy: scale.y,
            tx: check("offset x", offset_x, -MAX_COORD, MAX_COORD)?,
            ty: check("offset y", offset_y, -MAX_COORD, MAX_COORD)?,
            spaces: PhantomData,
        })
    }
    /// The same map in the other direction.
    pub fn inverse(&self) -> Map<B, A> {
        Map {
            sx: 1.0 / self.sx,
            sy: 1.0 / self.sy,
            tx: -self.tx / self.sx,
            ty: -self.ty / self.sy,
            spaces: PhantomData,
        }
    }
    /// This map followed by `next`.
    pub fn then<C>(&self, next: &Map<B, C>) -> Map<A, C> {
        Map {
            sx: self.sx * next.sx,
            sy: self.sy * next.sy,
            tx: self.tx * next.sx + next.tx,
            ty: self.ty * next.sy + next.ty,
            spaces: PhantomData,
        }
    }
    /// Maps a point. The result is not range-checked: chained maps of in-range inputs stay finite because scales are
    /// bounded, but can leave [`MAX_COORD`].
    pub fn pt(&self, p: Pt<A>) -> Pt<B> {
        Pt {
            x: p.x * self.sx + self.tx,
            y: p.y * self.sy + self.ty,
            space: PhantomData,
        }
    }
    /// Maps a rectangle (origin and size).
    pub fn rect(&self, r: Rect<A>) -> Rect<B> {
        Rect {
            x: r.x * self.sx + self.tx,
            y: r.y * self.sy + self.ty,
            w: r.w * self.sx,
            h: r.h * self.sy,
            space: PhantomData,
        }
    }
    /// Horizontal scale of the map.
    pub fn scale_x(&self) -> f64 {
        self.sx
    }
    /// Vertical scale of the map.
    pub fn scale_y(&self) -> f64 {
        self.sy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type R = Rect<Logical>;

    fn r(x: f64, y: f64, w: f64, h: f64) -> R {
        R::new(x, y, w, h).unwrap()
    }

    #[test]
    fn refuses_non_finite_and_out_of_range_input() {
        assert!(matches!(Pt::<Logical>::new(f64::NAN, 0.0), Err(GeometryError::NonFinite { .. })));
        assert!(matches!(
            Pt::<Logical>::new(0.0, f64::INFINITY),
            Err(GeometryError::NonFinite { .. })
        ));
        assert!(matches!(
            Pt::<Logical>::new(2.0 * MAX_COORD, 0.0),
            Err(GeometryError::OutOfRange { .. })
        ));
        assert!(matches!(R::new(0.0, 0.0, -1.0, 1.0), Err(GeometryError::OutOfRange { .. })));
        assert!(matches!(Scale::uniform(0.0), Err(GeometryError::OutOfRange { .. })));
        assert!(matches!(Scale::uniform(f64::NAN), Err(GeometryError::NonFinite { .. })));
        assert!(Scale::new(1.5, 0.5).is_ok());
    }

    #[test]
    fn intersection_touching_edges_is_none_but_touches_is_true() {
        let (a, b) = (r(0.0, 0.0, 10.0, 10.0), r(10.0, 0.0, 10.0, 10.0));
        assert!(a.intersection(&b).is_none());
        assert!(a.touches(&b, 0.0));
        assert!(!a.touches(&r(10.5, 0.0, 1.0, 1.0), 0.25));
        assert!(a.touches(&r(10.5, 0.0, 1.0, 1.0), 0.5));
    }

    #[test]
    fn iou_union_contains_center() {
        let (a, b) = (r(0.0, 0.0, 10.0, 10.0), r(5.0, 0.0, 10.0, 10.0));
        assert!((a.iou(&b) - 50.0 / 150.0).abs() < 1e-12);
        assert_eq!(a.iou(&r(20.0, 20.0, 1.0, 1.0)), 0.0);
        assert_eq!(r(0.0, 0.0, 0.0, 0.0).iou(&r(0.0, 0.0, 0.0, 0.0)), 0.0);
        assert_eq!(a.union(&b), r(0.0, 0.0, 15.0, 10.0));
        assert!(a.contains(&r(1.0, 1.0, 9.0, 9.0)) && !a.contains(&b));
        assert!(a.contains_pt(a.center()) && !a.contains_pt(Pt::new(10.1, 5.0).unwrap()));
        assert_eq!(a.clip(&b), Some(r(5.0, 0.0, 5.0, 10.0)));
        let c = Rect::<Logical>::from_corners(Pt::new(3.0, 4.0).unwrap(), Pt::new(1.0, 9.0).unwrap());
        assert_eq!((c.x(), c.y(), c.w(), c.h()), (1.0, 4.0, 2.0, 5.0));
    }

    #[test]
    fn map_composes_and_inverts() {
        let m: Map<Logical, RemotePx> = Map::new(Scale::uniform(1.5).unwrap(), 0.0, 0.0).unwrap();
        let n: Map<RemotePx, CapturePx> = Map::new(Scale::new(0.75, 0.5).unwrap(), 10.0, 20.0).unwrap();
        let both = m.then(&n);
        let p = both.pt(Pt::new(100.0, 40.0).unwrap());
        assert!((p.x() - (100.0 * 1.5 * 0.75 + 10.0)).abs() < 1e-9);
        assert!((p.y() - (40.0 * 1.5 * 0.5 + 20.0)).abs() < 1e-9);
        let back = both.inverse().pt(p);
        assert!((back.x() - 100.0).abs() < 1e-9 && (back.y() - 40.0).abs() < 1e-9);
        let rr = both.rect(r(2.0, 2.0, 8.0, 4.0));
        assert!((rr.w() - 8.0 * 1.5 * 0.75).abs() < 1e-9 && (rr.h() - 4.0 * 1.5 * 0.5).abs() < 1e-9);
        assert_eq!(format!("{:?}", Pt::<Logical>::new(1.0, 2.0).unwrap()), "Pt { x: 1.0, y: 2.0 }");
        assert!(both.scale_x() > 0.0 && both.scale_y() > 0.0);
    }
}
