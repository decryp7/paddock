//! Counts the bytes the current thread allocates, so a test can show that work does not copy. This module is the one `unsafe` of the
//! crate's tests: a global allocator is an `unsafe impl` by definition, and it only counts and forwards to the system allocator.
#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

thread_local! {
    static BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Forwards to the system allocator and adds every request to the calling thread's count.
pub struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = BYTES.try_with(|b| b.set(b.get().saturating_add(layout.size())));
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let _ = BYTES.try_with(|b| b.set(b.get().saturating_add(new_size)));
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// The bytes allocated on this thread while `f` ran.
pub fn allocated_by<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let before = BYTES.with(Cell::get);
    let r = f();
    (r, BYTES.with(Cell::get) - before)
}
