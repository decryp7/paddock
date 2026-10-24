import { VIEW_LIMITS, type Node, type ViewDoc, type ViewPatch } from '../generated/view';
import { validateViewDoc, type ViewIssue } from './limits';
import { shapeOf } from './model';

/** Why a patch was not applied (the TypeScript mirror of `paddock_view::PatchError`). `stale` means the host's document is not the one the patch was made against: ask the engine for the view again. */
export type PatchProblem =
  | { readonly kind: 'stale'; readonly have: number; readonly base: number; readonly message: string }
  | { readonly kind: 'sameEtag'; readonly etag: number; readonly message: string }
  | { readonly kind: 'unknownNode'; readonly id: string; readonly message: string }
  | { readonly kind: 'invalid'; readonly issue: ViewIssue; readonly message: string }
  | { readonly kind: 'malformed'; readonly message: string };

/** The result of applying a patch: the next document, or the problem (and the document unchanged). */
export type PatchResult = { readonly ok: true; readonly doc: ViewDoc } | { readonly ok: false; readonly problem: PatchProblem };

const fail = (problem: PatchProblem): PatchResult => ({ ok: false, problem });

/** Replaces the node with `id` (search stops below the depth limit) and returns a new tree that shares everything else; null when there is no such node. */
function replaceNode(node: Node, id: string, next: Node, depth: number): Node | null {
  if (id === '' || depth > VIEW_LIMITS.maxDepth) return null;
  if (node.id === id) return next;
  const s = shapeOf(node);
  if (s.tag !== 'stack') return null;
  for (let i = 0; i < s.v.children.length; i++) {
    const child = s.v.children[i];
    const replaced = child ? replaceNode(child, id, next, depth + 1) : null;
    if (replaced) {
      const children = s.v.children.slice();
      children[i] = replaced;
      return { ...node, kind: { stack: { ...s.v, children } } };
    }
  }
  return null;
}

/**
 * Applies a `ViewPatch` to a document, all or nothing, without changing `doc` (`apply_patch`): `{ replace: { baseEtag, etag, id, node } }`
 * needs `baseEtag` to be the document's etag and `etag` to differ from it, replaces the node with `id` (the root included) and
 * takes the new etag; `{ doc }` replaces the whole document and needs a different etag. The result must pass the limits.
 */
export function applyViewPatch(doc: ViewDoc, patch: unknown): PatchResult {
  if (patch === null || typeof patch !== 'object') return fail({ kind: 'malformed', message: $localize`:@@pd.view.patch.malformed:The update to the view could not be read.` });
  const p = patch as Partial<ViewPatch> & Record<string, unknown>;
  let next: unknown;
  if ('doc' in p && p.doc !== undefined) {
    const whole = p.doc as ViewDoc;
    if (whole?.etag === doc.etag) return fail({ kind: 'sameEtag', etag: doc.etag, message: $localize`:@@pd.view.patch.same:The update has the same version as the view on screen.` });
    next = whole;
  } else if ('replace' in p && p.replace && typeof p.replace === 'object') {
    const { baseEtag, etag, id, node } = p.replace as Extract<ViewPatch, { replace: unknown }>['replace'];
    if (baseEtag !== doc.etag) {
      return fail({ kind: 'stale', have: doc.etag, base: baseEtag, message: $localize`:@@pd.view.patch.stale:The view changed while you were working. Reload it to see the latest.` });
    }
    if (etag === baseEtag) return fail({ kind: 'sameEtag', etag, message: $localize`:@@pd.view.patch.same:The update has the same version as the view on screen.` });
    const root = replaceNode(doc.root, id, node, 1);
    if (!root) return fail({ kind: 'unknownNode', id, message: $localize`:@@pd.view.patch.unknown:The update names a part of the view that is not there ("${id}:id:").` });
    next = { ...doc, root, etag };
  } else {
    return fail({ kind: 'malformed', message: $localize`:@@pd.view.patch.malformed:The update to the view could not be read.` });
  }
  const issue = validateViewDoc(next);
  if (issue) return fail({ kind: 'invalid', issue, message: issue.message });
  return { ok: true, doc: next as ViewDoc };
}
