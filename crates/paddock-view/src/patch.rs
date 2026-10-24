//! Applying a [`ViewPatch`] to a document: all or nothing, and the result is validated.

use crate::model::{Node, NodeKind, ViewDoc, ViewPatch};
use crate::{MAX_DEPTH, PatchError, validate};

/// Applies `patch` to `doc`. On any error `doc` is unchanged.
///
/// - `Replace` needs `base_etag` to be the document's etag and `etag` to differ from it; the node with `id` (the root
///   included) is replaced by `node`, and the document gets the new etag.
/// - `Doc` replaces the whole document; its etag must differ from the current one.
///
/// The patched document must pass [`validate`].
pub fn apply_patch(doc: &mut ViewDoc, patch: ViewPatch) -> Result<(), PatchError> {
    let next = match patch {
        ViewPatch::Doc(new) => {
            if new.etag == doc.etag {
                return Err(PatchError::SameEtag { etag: new.etag });
            }
            new
        }
        ViewPatch::Replace {
            base_etag,
            etag,
            id,
            node,
        } => {
            if base_etag != doc.etag {
                return Err(PatchError::Stale {
                    have: doc.etag,
                    patch_base: base_etag,
                });
            }
            if etag == base_etag {
                return Err(PatchError::SameEtag { etag });
            }
            let mut next = doc.clone();
            let slot = find(&mut next.root, &id, 1).ok_or(PatchError::UnknownNode { id })?;
            *slot = node;
            next.etag = etag;
            next
        }
    };
    validate(&next)?;
    *doc = next;
    Ok(())
}

/// The node with this id (an empty id matches nothing). The search stops below [`MAX_DEPTH`].
fn find<'a>(node: &'a mut Node, id: &str, depth: usize) -> Option<&'a mut Node> {
    if id.is_empty() || depth > MAX_DEPTH {
        return None;
    }
    if node.id == id {
        return Some(node);
    }
    match &mut node.kind {
        NodeKind::Stack { children, .. } => {
            children.iter_mut().find_map(|c| find(c, id, depth + 1))
        }
        _ => None,
    }
}
