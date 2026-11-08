//! The AI engine (ADR 0059, ADR 0070): Pitwall, the family's local model server, is the only one. Every feature reaches
//! the model through the `LlmProvider` port ([`pitwall::PitwallProvider`] is its adapter).

pub mod pitwall;
