//! A counting global allocator for the test binary: how many heap allocations the calling thread made. The crash model's hot
//! loop is bounded by the allocator on Windows (the process heap is some 10 times dearer per call than macOS's), so its
//! allocations per edge are measured and held to a budget (see `model.rs`) instead of guessed.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static ALLOCS: Cell<u64> = const { Cell::new(0) };
}

struct Counting;

// SAFETY: every call is forwarded unchanged to the system allocator; the counter is a thread-local `Cell` with a const
// initialiser and no destructor, so reading it inside the allocator never allocates or re-enters.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let _ = ALLOCS.try_with(|c| c.set(c.get() + 1));
        unsafe { System.realloc(p, l, n) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// The allocations (allocate, zeroed allocate and grow) the calling thread has made so far.
pub fn allocations() -> u64 {
    ALLOCS.try_with(Cell::get).unwrap_or(0)
}
