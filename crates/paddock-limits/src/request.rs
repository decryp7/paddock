//! Request-side types that plan 2.5 keeps (ADR 0018): a page size and a bucket count that cannot be zero.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::num::{NonZeroU16, NonZeroU32};

/// A request type was zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestError(&'static str);

impl fmt::Display for RequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} must be at least 1", self.0)
    }
}

impl std::error::Error for RequestError {}

/// How many items a request asks for; never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Limit(NonZeroU32);

impl Limit {
    /// A limit.
    ///
    /// # Errors
    /// [`RequestError`] for zero.
    pub fn new(n: u32) -> Result<Self, RequestError> {
        NonZeroU32::new(n).map(Self).ok_or(RequestError("limit"))
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0.get()
    }
}

/// A page: where to start and how many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS))]
pub struct Page {
    /// Items to skip.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub offset: u64,
    /// Items to return.
    pub limit: Limit,
}

/// How many buckets a histogram or timeline request asks for; never zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Buckets(NonZeroU16);

impl Buckets {
    /// A bucket count.
    ///
    /// # Errors
    /// [`RequestError`] for zero.
    pub fn new(n: u16) -> Result<Self, RequestError> {
        NonZeroU16::new(n).map(Self).ok_or(RequestError("buckets"))
    }

    /// The number.
    #[must_use]
    pub const fn get(self) -> u16 {
        self.0.get()
    }
}

/// `number` in TypeScript for a newtype over a non-zero integer.
#[cfg(feature = "ts")]
macro_rules! ts_number {
    ($t:ident) => {
        impl ts_rs::TS for $t {
            type WithoutGenerics = Self;
            type OptionInnerType = Self;
            fn name() -> String {
                stringify!($t).into()
            }
            fn inline() -> String {
                "number".into()
            }
            fn inline_flattened() -> String {
                "number".into()
            }
            fn decl() -> String {
                concat!("type ", stringify!($t), " = number;").into()
            }
            fn decl_concrete() -> String {
                concat!("type ", stringify!($t), " = number;").into()
            }
        }
    };
}
#[cfg(feature = "ts")]
ts_number!(Limit);
#[cfg(feature = "ts")]
ts_number!(Buckets);
