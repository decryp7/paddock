//! Frame handling (SG2), pure apart from the PNG codec: perceptual hashes, SSIM, scene change and near-duplicate removal.
//! Ported from the proof of concept (`poc/vision/src/phash.rs`) with tests rewritten to generated images; the hash is
//! total (any valid [`sighter_core::Frame`] gives a hash) and the PNG decoder enforces the frame limits before it allocates.

pub mod downscale;
pub mod hash;
pub mod png_io;
pub mod scene;

pub use hash::{CELL_TOLERANCE, DHash, GRID_H, GRID_W, GridHash};
pub use png_io::{PngError, decode_png, encode_png};
pub use scene::{SCENE_CHANGE_CELLS, dedupe, is_scene_change, ssim};
