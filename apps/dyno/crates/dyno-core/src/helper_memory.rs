//! The helper's `MemoryModules` reader port (ADR 0027 WP-6): the slot table needs root on Linux (`/sys/firmware/dmi/tables/DMI`).
//! The answer is the family's own [`MemoryLayout`], which has no serial number field, so none can leave the helper.

use crate::inventory::{MemoryLayout, Unavailable};

/// Reads the machine's memory slots.
pub trait MemoryReader: Send + Sync {
    /// The layout now; a failure is a `TotalOnly` with the reason, never a panic.
    fn read(&self) -> MemoryLayout;
}

/// What a system that reads this without the helper says.
pub struct NoMemory;

impl MemoryReader for NoMemory {
    fn read(&self) -> MemoryLayout {
        MemoryLayout::TotalOnly {
            total: 0,
            reason: Unavailable::failed("This system's memory slots are read without the helper."),
        }
    }
}
