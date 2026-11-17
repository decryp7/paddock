//! This process's environment as text: [`paddock_env::vars`] (the one implementation, skipping a variable that is not valid
//! Unicode instead of panicking like `std::env::vars`, which Clippy forbids).

pub use paddock_env::{vars, vars_skipping as skipping};
