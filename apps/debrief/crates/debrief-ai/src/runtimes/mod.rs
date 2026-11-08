//! The pinned-release catalog and the GitHub release source that the FFmpeg runtime and the research browser's download
//! use (ADR 0070 D5; both leave with the move onto `paddock-download`).
//!
//! - [`catalog`]: a pinned release and its assets per platform;
//! - [`github`]: fetching those assets from GitHub releases (a [`crate::models::ModelSource`]);
//! - [`builds`]: the build-id check.

pub mod builds;
pub mod catalog;
pub mod github;

pub use catalog::{current_platform, RuntimeAsset, RuntimeBuild, RuntimeCatalog};
pub use github::{GithubReleases, ReleaseConfig};
