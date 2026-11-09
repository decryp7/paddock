//! ADR 0060, "every check runs somewhere": C100, the meta-check over checks, CI and the docs that name them.
//!
//! The pure core works on file contents (`yaml`, `expr`, `ci`, later the enumerators and the register); only the edge reads
//! the checkout. Everything the model cannot resolve is reported as unresolved, never as gated (ER-33).

pub mod checks;
pub mod ci;
pub mod command;
pub mod docs;
pub mod edge;
pub mod evaluate;
pub mod expr;
pub mod gating;
pub mod inputs;
pub mod register;
pub mod scan;
pub mod shell;
pub mod tree;
pub mod yaml;
