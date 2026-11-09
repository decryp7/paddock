//! Images for the one local model (ADR 0025): the same model reads text and images when its multimodal
//! projector is loaded with it.
//!
//! - untrusted images and their EXIF capture time are `debrief-image`'s (ADR 0070 D5);
//! - [`facts`]: what the model reads off a screenshot, as strict, bounded data;
//! - [`assist`]: explaining an image and "find related logs" (the model reads, the engine searches,
//!   `debrief_core::related`, the model explains the matches);
//! - [`intent`]: whether a chat question with an image asks for its related logs.

pub mod assist;
pub mod facts;
pub mod intent;

pub use assist::{RelatedOutcome, RelatedStage, MAX_IMAGES_PER_REQUEST};
pub use facts::{ScreenFacts, ScreenKind};
