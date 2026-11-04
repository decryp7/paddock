//! One integration-test binary (new-crate skill step 5): the contract between the library's modules and, on Windows, the
//! real program against real Win32 objects.

mod contract;
#[cfg(windows)]
mod windows;
