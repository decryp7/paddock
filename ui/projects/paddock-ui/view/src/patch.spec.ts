import { describe, expect, it } from 'vitest';
import type { Node, ViewDoc } from '../generated/view';
import { shapeOf } from './model';
import { applyViewPatch } from './patch';
import { sampleViewDoc } from './sample';

const text = (id: string, t: string): Node => ({ id, kind: { text: { text: t, tone: 'neutral', style: 'body' } } });
const small = (etag = 1): ViewDoc => ({
  schema: 1,
  title: 'T',
  etag,
  root: { id: 'root', kind: { stack: { dir: 'column', children: [text('a', 'one'), { id: 'inner', kind: { stack: { dir: 'row', children: [text('b', 'two')] } } }] } } },
});
const textOf = (doc: ViewDoc, path: number[]): string => {
  let n = doc.root;
  for (const i of path) {
    const s = shapeOf(n);
    if (s.tag !== 'stack') throw new Error('not a stack');
    n = s.v.children[i] as Node;
  }
  const s = shapeOf(n);
  return s.tag === 'text' ? s.v.text : '';
};

describe('applyViewPatch (apply_patch)', () => {
  it('replaces a node by id, takes the new etag and leaves the old document alone', () => {
    const doc = small();
    const r = applyViewPatch(doc, { replace: { baseEtag: 1, etag: 2, id: 'b', node: text('b', 'changed') } });
    expect(r.ok).toBe(true);
    if (!r.ok) return;
    expect(r.doc.etag).toBe(2);
    expect(textOf(r.doc, [1, 0])).toBe('changed');
    expect(textOf(doc, [1, 0])).toBe('two');
    // Everything that did not change is shared.
    expect((r.doc.root.kind as { stack: { children: Node[] } }).stack.children[0]).toBe((doc.root.kind as { stack: { children: Node[] } }).stack.children[0]);
  });

  it('replaces the root', () => {
    const r = applyViewPatch(small(), { replace: { baseEtag: 1, etag: 5, id: 'root', node: text('root', 'only') } });
    expect(r.ok && r.doc.root.id).toBe('root');
    expect(r.ok && textOf(r.doc, [])).toBe('only');
  });

  it('refuses a stale base, an unchanged etag and an unknown or empty id, and says what to do', () => {
    const stale = applyViewPatch(small(3), { replace: { baseEtag: 2, etag: 4, id: 'b', node: text('b', 'x') } });
    expect(!stale.ok && stale.problem).toMatchObject({ kind: 'stale', have: 3, base: 2 });
    expect(!stale.ok && stale.problem.message).toContain('Reload');
    const same = applyViewPatch(small(), { replace: { baseEtag: 1, etag: 1, id: 'b', node: text('b', 'x') } });
    expect(!same.ok && same.problem.kind).toBe('sameEtag');
    const unknown = applyViewPatch(small(), { replace: { baseEtag: 1, etag: 2, id: 'zz', node: text('zz', 'x') } });
    expect(!unknown.ok && unknown.problem).toMatchObject({ kind: 'unknownNode', id: 'zz' });
    const empty = applyViewPatch(small(), { replace: { baseEtag: 1, etag: 2, id: '', node: text('', 'x') } });
    expect(!empty.ok && empty.problem.kind).toBe('unknownNode');
    // A node that is not inside a stack is not searched.
    const leaf = applyViewPatch({ ...small(), root: text('root', 'x') }, { replace: { baseEtag: 1, etag: 2, id: 'b', node: text('b', 'y') } });
    expect(!leaf.ok && leaf.problem.kind).toBe('unknownNode');
  });

  it('refuses a result that breaks the limits', () => {
    const dup = applyViewPatch(small(), { replace: { baseEtag: 1, etag: 2, id: 'b', node: text('a', 'dup') } });
    expect(!dup.ok && dup.problem.kind).toBe('invalid');
    expect(!dup.ok && dup.problem.kind === 'invalid' && dup.problem.issue.code).toBe('duplicateId');
  });

  it('replaces the whole document when the etag differs', () => {
    const r = applyViewPatch(small(), { doc: sampleViewDoc(9) });
    expect(r.ok && r.doc.etag).toBe(9);
    const same = applyViewPatch(small(9), { doc: sampleViewDoc(9) });
    expect(!same.ok && same.problem.kind).toBe('sameEtag');
    const bad = applyViewPatch(small(), { doc: { ...sampleViewDoc(9), schema: 7 } });
    expect(!bad.ok && bad.problem.kind).toBe('invalid');
  });

  it('refuses anything that is not a patch', () => {
    for (const p of [null, undefined, 5, 'x', {}, { replace: null }, { other: 1 }]) {
      const r = applyViewPatch(small(), p);
      expect(!r.ok && r.problem.kind).toBe('malformed');
    }
  });
});
