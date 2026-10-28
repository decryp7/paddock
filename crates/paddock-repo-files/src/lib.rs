//! Repository files: a bounded walk and hashing (ADR 0022 §6; the layer `paddock-repo-graph` and, later, Pacenotes'
//! index stand on).
//!
//! - [`walk`]: every file below a root, `.gitignore`-aware (also outside a git repository), `.git` skipped, hidden files
//!   kept, symbolic links not followed, bounded by [`WalkLimits`]; paths are `/`-separated and relative, the list sorted.
//! - [`hash_file`], [`hash_bytes`], [`hash_files`], [`hash_listing`]: SHA-256 as lowercase hex; file hashing streams and is
//!   capped; [`hash_files`] reads on a bounded number of threads and returns the same order whatever the thread count.
//!
//! Pure apart from reading the folder it is given; nothing is written.

mod error;
mod hash;
mod walk;

pub use error::RepoFilesError;
pub use hash::{hash_bytes, hash_file, hash_files, hash_listing};
pub use walk::{RepoFile, WalkLimits, walk};
