//! Pitwall's example filter plugin: redacts e-mail addresses, IPv4 addresses and long hex/base64 secrets from the
//! messages of every request before they reach the model ([`redact`]), and, when the user turns it on, appends one
//! line of counts (never content) per request to `audit.log` in its data folder ([`audit`]).
//!
//! Settings (a JSON object): `{"audit": true}`.

// Tests may unwrap: a failed unwrap is a failed test.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod audit;
mod filter;
mod redact;

fn main() -> std::process::ExitCode {
    pitwall_plugin_api::serve(filter::plugin())
}
