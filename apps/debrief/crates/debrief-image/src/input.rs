//! The image handed to a model: only [`crate::sanitize`] makes one (ADR 0025).

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// How a sanitised image is encoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageMime {
    /// PNG (`image/png`).
    Png,
    /// JPEG (`image/jpeg`).
    Jpeg,
}

impl ImageMime {
    /// The MIME type as the provider's API names it.
    pub fn as_str(self) -> &'static str {
        match self {
            ImageMime::Png => "image/png",
            ImageMime::Jpeg => "image/jpeg",
        }
    }
}

/// An image for the model. Only [`crate::sanitize`] makes one outside tests: decoded in Rust within limits,
/// downscaled and re-encoded without metadata, so a provider never sees the user's raw bytes (ADR 0025).
#[derive(Debug, Clone, PartialEq)]
pub struct ImageInput {
    mime: ImageMime,
    bytes: Arc<[u8]>,
    width: u32,
    height: u32,
}

impl ImageInput {
    #[doc(hidden)]
    pub fn new(mime: ImageMime, bytes: Vec<u8>, width: u32, height: u32) -> Self {
        ImageInput {
            mime,
            bytes: bytes.into(),
            width,
            height,
        }
    }

    /// How the image is encoded.
    pub fn mime(&self) -> ImageMime {
        self.mime
    }

    /// The encoded image (PNG or JPEG).
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}
