//! The typed errors of the view model: an invalid document ([`ViewError`]) and a patch that cannot be applied
//! ([`PatchError`]). Both name the node path, so a plugin author sees which node broke which limit.

use std::fmt;

/// Why a [`crate::ViewDoc`] (or a [`crate::ViewAction`]) is refused. `path` is the node path, e.g.
/// `root.children[2].rows[0][1]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewError {
    /// The bytes or text do not decode as the expected type (or are larger than the encoded limit).
    Decode(String),
    /// The document's `schema` is not one this build renders.
    UnsupportedSchema { found: u16, supported: u16 },
    /// Nodes are nested deeper than [`crate::MAX_DEPTH`].
    TooDeep { path: String },
    /// The document has more than [`crate::MAX_NODES`] nodes.
    TooManyNodes { path: String },
    /// The encoded document is larger than [`crate::MAX_ENCODED`] bytes.
    TooLarge { bytes: usize },
    /// A string is longer than [`crate::MAX_STRING`] bytes.
    StringTooLong { path: String, len: usize },
    /// A graph or diagram is larger than its limits.
    GraphTooLarge { path: String, nodes: usize, edges: usize },
    /// A table has too many inline rows (use `Paged`).
    TooManyRows { path: String, rows: usize },
    /// An interactive node has no text label.
    Unlabelled { path: String },
    /// A link is not an `http` or `https` URL.
    BadUrl { path: String },
    /// Two nodes (or tree nodes, graph nodes, ...) share an id.
    DuplicateId { path: String, id: String },
    /// Anything else that cannot be rendered (a bad reference, a number out of range, ...).
    Invalid { path: String, reason: String },
}

impl fmt::Display for ViewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ViewError::Decode(m) => write!(f, "the view does not decode: {m}"),
            ViewError::UnsupportedSchema { found, supported } => {
                write!(f, "view schema {found} is not supported (this build renders schema {supported})")
            }
            ViewError::TooDeep { path } => write!(f, "{path}: nested deeper than {} levels", crate::MAX_DEPTH),
            ViewError::TooManyNodes { path } => write!(f, "{path}: more than {} nodes in one view", crate::MAX_NODES),
            ViewError::TooLarge { bytes } => write!(f, "the view is {bytes} bytes encoded; the limit is {}", crate::MAX_ENCODED),
            ViewError::StringTooLong { path, len } => {
                write!(f, "{path}: a string of {len} bytes; the limit is {}", crate::MAX_STRING)
            }
            ViewError::GraphTooLarge { path, nodes, edges } => write!(
                f,
                "{path}: {nodes} nodes and {edges} edges; the limits are {} and {}",
                crate::MAX_GRAPH_NODES,
                crate::MAX_GRAPH_EDGES
            ),
            ViewError::TooManyRows { path, rows } => {
                write!(f, "{path}: {rows} inline rows; the limit is {} (use paged rows)", crate::MAX_INLINE_ROWS)
            }
            ViewError::Unlabelled { path } => write!(f, "{path}: an interactive node needs a text label"),
            ViewError::BadUrl { path } => write!(f, "{path}: only http and https links can be opened"),
            ViewError::DuplicateId { path, id } => write!(f, "{path}: the id \"{id}\" is used twice"),
            ViewError::Invalid { path, reason } => write!(f, "{path}: {reason}"),
        }
    }
}

impl std::error::Error for ViewError {}

/// Why a [`crate::ViewPatch`] was not applied. The document is unchanged after any of these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchError {
    /// The patch was made against another version of the document; the host asks for the whole view again.
    Stale { have: u64, patch_base: u64 },
    /// The patch does not move the etag forward (it must name a new one).
    SameEtag { etag: u64 },
    /// No node has this id.
    UnknownNode { id: String },
    /// The patched document would be invalid.
    Invalid(ViewError),
}

impl fmt::Display for PatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchError::Stale { have, patch_base } => {
                write!(f, "the patch is for version {patch_base} of the view; this is version {have}")
            }
            PatchError::SameEtag { etag } => write!(f, "the patch keeps etag {etag}; it must name a new one"),
            PatchError::UnknownNode { id } => write!(f, "no node has the id \"{id}\""),
            PatchError::Invalid(e) => write!(f, "the patched view is invalid: {e}"),
        }
    }
}

impl std::error::Error for PatchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PatchError::Invalid(e) => Some(e),
            _ => None,
        }
    }
}

impl From<ViewError> for PatchError {
    fn from(e: ViewError) -> Self {
        PatchError::Invalid(e)
    }
}
