//! Integration tests of the `wiki` tool: the lint's cases (ported from the Node lint's 18), the command line, the writing
//! commands, hostile input, and the parity golden against the retired Node implementation's recorded findings.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod cli_cases;
mod common;
mod golden;
mod hostile;
mod lint_cases;
