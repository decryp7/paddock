//! Debrief's AI layer. Inference runs in Pitwall, the family's local model server (ADR 0070); nothing about the logs
//! leaves the machine (web pages reach the model only when the user hands them over from the internal browser, as
//! untrusted data). The LLM only proposes: every proposal is validated and previewed by `debrief-core` before the user
//! can apply it.

pub mod access;
pub mod assistant;
pub mod browser;
pub mod chat;
pub mod engine;
pub mod error;
pub mod knowledge;
pub mod llm;
pub mod models;
pub mod net;
pub mod runtimes;
pub mod settings;
pub mod vision;
pub mod web;

#[cfg(test)]
mod removed_phrases;

pub use access::{StoreAccess, StoreRef};
pub use error::{AiError, Result};
