//! Untrusted images, made safe for a model and the webview (ADR 0025, ADR 0070 D5): decoded in Rust within limits as
//! PNG, JPEG, BMP, GIF (first frame) or WebP only, downscaled, re-encoded from the pixels, with the EXIF capture time
//! as the one fact kept. Pure: no I/O, no AI. Notes and video use it without the AI; `debrief-ai` depends on it, never
//! the reverse.

pub mod exif;
pub mod input;
pub mod sanitize;

pub use input::{ImageInput, ImageMime};
pub use sanitize::{sanitize, testing, ImageError, ImageLimits, SanitizedImage, SourceFormat, DEFAULT_TARGET_PIXELS, MAX_OUTPUT_SIDE};
