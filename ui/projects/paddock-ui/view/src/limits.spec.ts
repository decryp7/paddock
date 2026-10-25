import { describe, expect, it } from 'vitest';
import { VIEW_LIMITS, type Node, type NodeKind, type ViewDoc } from '../generated/view';
import { VIEW_EXTRA_LIMITS, isWebUrl, validateViewAction, validateViewDoc, type ViewIssue } from './limits';
import { sampleViewDoc } from './sample';

const doc = (kind: NodeKind, over: Partial<ViewDoc> = {}): ViewDoc => ({ schema: 1, title: 'T', etag: 1, root: { id: '', kind }, ...over });
const text = (t = 'x'): NodeKind => ({ text: { text: t, tone: 'neutral', style: 'body' } });
const stack = (children: Node[]): NodeKind => ({ stack: { dir: 'column', children } });
const act = (over: Record<string, unknown> = {}) => ({ id: 'a', label: 'Go', confirm: null, args: null, writes: false, ...over });
const issue = (d: unknown): ViewIssue => {
  const i = validateViewDoc(d);
  if (!i) throw new Error('expected an issue');
  return i;
};
const table = (over: Record<string, unknown> = {}): NodeKind =>
  ({ table: { columns: [{ key: 'a', label: 'A', sortable: false, align: 'start', unit: '' }], rows: { inline: [] }, rowAction: null, sort: null, ...over } }) as unknown as NodeKind;

describe('validateViewDoc: the mirror of paddock_view::validate', () => {
  it('accepts the sample document with a node of every kind', () => {
    expect(validateViewDoc(sampleViewDoc())).toBeNull();
  });

  it('does not show a document that makes the check itself fail', () => {
    const hostile = { get schema(): number { throw new Error('boom'); } };
    expect(issue(hostile).message).toContain('could not be read');
    const action = { get view(): string { throw new Error('boom'); } } as unknown as { view: string; action: string; args: unknown };
    expect(validateViewAction(action)?.code).toBe('invalid');
  });

  it('refuses what is not a document, another schema and an unsafe etag', () => {
    expect(issue(null).code).toBe('notADocument');
    expect(issue([]).code).toBe('notADocument');
    const other = issue({ ...doc(text()), schema: 2 });
    expect(other.code).toBe('unsupportedSchema');
    expect(other.message).toContain('Update the app');
    expect(issue(doc(text(), { etag: 2 ** 53 })).code).toBe('invalid');
    expect(issue(doc(text(), { etag: -1 })).code).toBe('invalid');
    expect(issue(doc(text(), { etag: 1.5 })).code).toBe('invalid');
    expect(issue(doc(text(), { title: ' ' })).code).toBe('unlabelled');
  });

  it('refuses a document nested deeper than the limit and counts the root as level 1', () => {
    let kind: NodeKind = text();
    for (let i = 0; i < VIEW_LIMITS.maxDepth - 1; i++) kind = stack([{ id: '', kind }]);
    expect(validateViewDoc(doc(kind))).toBeNull();
    const deep = issue(doc(stack([{ id: '', kind }])));
    expect(deep.code).toBe('tooDeep');
    expect(deep.path).toContain('children[0]');
  });

  it('refuses more nodes than the limit, counting rows, tree nodes and cells too', () => {
    const many = Array.from({ length: VIEW_LIMITS.maxNodes }, () => ({ id: '', kind: text() }));
    expect(issue(doc(stack(many))).code).toBe('tooManyNodes');
    const rows = Array.from({ length: VIEW_LIMITS.maxInlineRows }, () => [{ text: 'x', tone: 'neutral', link: null }]);
    const tables = Array.from({ length: 11 }, (_, i) => ({ id: `t${i}`, kind: table({ rows: { inline: rows } }) }));
    expect(issue(doc(stack(tables))).code).toBe('tooManyNodes');
  });

  it('refuses long strings, in bytes, and empty labels', () => {
    expect(issue(doc(text('é'.repeat(VIEW_LIMITS.maxStringBytes / 2 + 1)))).code).toBe('stringTooLong');
    expect(validateViewDoc(doc(text('é'.repeat(VIEW_LIMITS.maxStringBytes / 2))))).toBeNull();
    expect(issue(doc({ badge: { text: '  ', tone: 'neutral' } })).code).toBe('unlabelled');
    expect(issue(doc({ text: { text: 5, tone: 'neutral', style: 'body' } } as unknown as NodeKind)).code).toBe('invalid');
  });

  it('refuses ids that are too long or repeat, and lets empty ids repeat', () => {
    expect(issue(doc(stack([{ id: 'a', kind: text() }, { id: 'a', kind: text() }]))).code).toBe('duplicateId');
    expect(validateViewDoc(doc(stack([{ id: '', kind: text() }, { id: '', kind: text() }])))).toBeNull();
    expect(issue(doc(text(), { root: { id: 'x'.repeat(VIEW_EXTRA_LIMITS.maxId + 1), kind: text() } })).code).toBe('invalid');
  });

  it('refuses a document larger than the limit when encoded', () => {
    const per = Array.from({ length: 140 }, (_, i) => ({ id: `n${i}`, kind: text('y'.repeat(VIEW_LIMITS.maxStringBytes - 1)) }));
    expect(issue(doc(stack(per))).code).toBe('tooLarge');
  });

  it('refuses a malformed shape with the path of the node', () => {
    expect(issue({ ...doc(text()), root: 5 }).code).toBe('invalid');
    expect(issue({ ...doc(text()), root: { id: 1, kind: text() } }).code).toBe('invalid');
    expect(issue({ ...doc(text()), root: { id: '', kind: 5 } }).code).toBe('invalid');
    expect(issue(doc({ stack: { dir: 'row', children: 'no' } } as unknown as NodeKind)).message).toContain('list');
    expect(issue(doc({ progress: { value: 'x', label: '' } } as unknown as NodeKind)).message).toContain('number');
    expect(issue(doc({ tree: { nodes: [], lazy: 1 } } as unknown as NodeKind)).message).toContain('true or false');
    expect(issue(doc({ text: 5 } as unknown as NodeKind)).code).toBe('invalid');
  });

  it('lets a kind it does not know through as one node', () => {
    expect(validateViewDoc(doc({ hologram: { depth: 3 } } as unknown as NodeKind))).toBeNull();
    expect(validateViewDoc(doc({} as unknown as NodeKind))).toBeNull();
  });

  it('checks progress, empty, needs-AI and link', () => {
    expect(validateViewDoc(doc({ progress: { value: 0, label: '' } }))).toBeNull();
    expect(validateViewDoc(doc({ progress: { value: 1, label: '' } }))).toBeNull();
    expect(issue(doc({ progress: { value: 1.01, label: '' } })).code).toBe('invalid');
    expect(issue(doc({ progress: { value: Number.NaN, label: '' } })).code).toBe('invalid');
    expect(issue(doc({ empty: { icon: 'info', title: '', hint: '', action: null } })).code).toBe('unlabelled');
    expect(issue(doc({ empty: { icon: 'info', title: 'T', hint: '', action: act({ label: '' }) } })).code).toBe('unlabelled');
    expect(issue(doc({ needsAi: { feature: 'F', reason: '', setup: act({ confirm: ' ' }) } })).code).toBe('unlabelled');
    expect(issue(doc({ link: { label: '', target: { action: act() } } })).code).toBe('unlabelled');
  });

  it('allows only web addresses without user information in links', () => {
    const link = (url: string): NodeKind => ({ link: { label: 'L', target: { openInBrowser: { url, label: 'L' } } } });
    expect(validateViewDoc(doc(link('https://example.com/a?b#c')))).toBeNull();
    expect(validateViewDoc(doc(link('HTTP://example.com')))).toBeNull();
    for (const bad of ['javascript:alert(1)', 'file:///etc/passwd', 'https://user@example.com/', 'https:///x', 'https://', 'https://exa mple.com', 'https://:80/', 'ftp://example.com']) {
      expect(issue(doc(link(bad))).code).toBe('badUrl');
    }
    expect(issue(doc(link(`https://example.com/${'a'.repeat(VIEW_EXTRA_LIMITS.maxUrl)}`))).code).toBe('stringTooLong');
    expect(isWebUrl('https://example.com')).toBe(true);
    expect(isWebUrl('https://example.com/\u0007')).toBe(false);
  });

  it('refuses too large arguments of an action', () => {
    const big = { blob: 'x'.repeat(VIEW_EXTRA_LIMITS.maxArgsBytes) };
    expect(issue(doc({ empty: { icon: 'info', title: 'T', hint: '', action: act({ args: big }) } })).code).toBe('invalid');
    const cyc: Record<string, unknown> = {};
    cyc['self'] = cyc;
    expect(issue(doc({ empty: { icon: 'info', title: 'T', hint: '', action: act({ args: cyc }) } })).code).toBe('invalid');
  });

  it('checks tables: columns, sort, cell counts, row limits and pages', () => {
    const cell = { text: 'x', tone: 'neutral', link: null };
    expect(validateViewDoc(doc(table({ rows: { inline: [[cell]] } })))).toBeNull();
    expect(issue(doc(table({ columns: [{ key: 'a', label: 'A', sortable: false, align: 'start', unit: '' }, { key: 'a', label: 'B', sortable: false, align: 'start', unit: '' }] }))).code).toBe('duplicateId');
    expect(issue(doc(table({ sort: { column: 'zz', descending: false } }))).message).toContain('no column');
    expect(issue(doc(table({ rows: { inline: [[cell, cell]] } }))).message).toContain('2 cells for 1 columns');
    expect(issue(doc(table({ rows: { inline: Array.from({ length: VIEW_LIMITS.maxInlineRows + 1 }, () => [cell]) } }))).code).toBe('tooManyRows');
    expect(issue(doc(table({ rowAction: act({ id: '' }) }))).code).toBe('invalid');
    const paged = (p: Record<string, unknown>): ViewDoc => doc(table({ rows: { paged: { total: 10, pageSize: 5, page: 0, rows: [], ...p } } }));
    expect(validateViewDoc(paged({ page: 1, rows: [[cell], [cell]] }))).toBeNull();
    expect(validateViewDoc(paged({ total: 0 }))).toBeNull();
    expect(issue(paged({ pageSize: 0 })).code).toBe('invalid');
    expect(issue(paged({ pageSize: VIEW_LIMITS.maxInlineRows + 1 })).code).toBe('invalid');
    expect(issue(paged({ rows: Array.from({ length: 6 }, () => [cell]) })).code).toBe('tooManyRows');
    expect(issue(paged({ page: 2 })).message).toContain('beyond');
    expect(issue(paged({ total: 1, rows: [[cell], [cell]] })).message).toContain('more rows');
    expect(issue(paged({ total: 2 ** 53 })).code).toBe('invalid');
    expect(issue(doc(table({ rows: { inline: [[{ text: 'x', tone: 'neutral', link: { action: act({ id: '' }) } }]] } }))).code).toBe('invalid');
  });

  it('checks trees: depth steps, laziness and the nesting limit', () => {
    const tn = (id: string, depth: number, expandable = false) => ({ id, label: id, depth, tone: 'neutral', icon: null, expandable, action: null });
    const tree = (nodes: unknown[], lazy = true): NodeKind => ({ tree: { nodes, lazy } }) as unknown as NodeKind;
    expect(validateViewDoc(doc(tree([tn('a', 0), tn('b', 1), tn('c', 0)])))).toBeNull();
    expect(issue(doc(tree([tn('a', 1)]))).message).toContain('one level');
    expect(issue(doc(tree([tn('a', 0), tn('b', 2)]))).message).toContain('one level');
    expect(issue(doc(tree([tn('a', 0), tn('a', 0)]))).code).toBe('duplicateId');
    expect(issue(doc(tree([tn('a', 0, true)], false))).message).toContain('lazy');
    const chain = Array.from({ length: VIEW_LIMITS.maxDepth }, (_, i) => tn(`n${i}`, i));
    expect(issue(doc(tree(chain))).code).toBe('tooDeep');
  });

  it('checks graphs and diagrams: sizes, ids and the nodes that edges name', () => {
    const gn = (id: string) => ({ id, label: id, tone: 'neutral', action: null });
    const graph = (nodes: unknown[], edges: unknown[]): NodeKind => ({ graph: { nodes, edges, layout: 'layered' } }) as unknown as NodeKind;
    expect(validateViewDoc(doc(graph([gn('a'), gn('b')], [{ from: 'a', to: 'b', label: '' }])))).toBeNull();
    expect(issue(doc(graph([gn('a')], [{ from: 'a', to: 'zz', label: '' }]))).message).toContain('no node');
    expect(issue(doc(graph([gn('a'), gn('a')], []))).code).toBe('duplicateId');
    expect(issue(doc(graph(Array.from({ length: VIEW_LIMITS.maxGraphNodes + 1 }, (_, i) => gn(`n${i}`)), []))).code).toBe('graphTooLarge');
    const edges = Array.from({ length: VIEW_LIMITS.maxGraphEdges + 1 }, () => ({ from: 'a', to: 'a', label: '' }));
    expect(issue(doc(graph([gn('a')], edges))).code).toBe('graphTooLarge');
    const el = (id: string) => ({ id, label: id, detail: '', group: '' });
    const diagram = (elements: unknown[], relations: unknown[]): NodeKind => ({ diagram: { kind: 'flow', model: { elements, relations } } }) as unknown as NodeKind;
    expect(validateViewDoc(doc(diagram([el('a')], [{ from: 'a', to: 'a', label: 'x' }])))).toBeNull();
    expect(issue(doc(diagram([el('a'), el('a')], []))).code).toBe('duplicateId');
    expect(issue(doc(diagram([el('a')], [{ from: 'q', to: 'a', label: '' }]))).message).toContain('no node');
    expect(issue(doc(diagram([{ ...el('a'), label: '' }], []))).code).toBe('unlabelled');
  });

  it('checks forms: every field kind and its value rules', () => {
    const form = (fields: unknown[]): NodeKind => ({ form: { fields, submit: act() } }) as unknown as NodeKind;
    const field = (id: string, kind: unknown) => ({ id, label: id, help: '', required: false, kind });
    expect(validateViewDoc(doc(form([field('a', { text: { value: '', placeholder: '', multiline: false } }), field('b', { toggle: { value: false } })])))).toBeNull();
    expect(issue(doc(form([field('a', { toggle: { value: false } }), field('a', { toggle: { value: false } })]))).code).toBe('duplicateId');
    expect(issue(doc(form([field('a', { toggle: { value: 1 } })]))).code).toBe('invalid');
    const number = (n: Record<string, unknown>) => doc(form([field('n', { number: { value: 5, min: null, max: null, ...n } })]));
    expect(validateViewDoc(number({ min: 0, max: 10 }))).toBeNull();
    expect(issue(number({ min: 6 })).message).toContain('outside');
    expect(issue(number({ max: 4 })).message).toContain('outside');
    expect(issue(number({ min: 9, max: 1, value: 5 })).message).toContain('outside');
    expect(issue(number({ value: Number.POSITIVE_INFINITY })).code).toBe('invalid');
    const choice = (value: string, extra: unknown[] = []) => doc(form([field('c', { choice: { value, options: [{ value: 'x', label: 'X' }, ...extra] } })]));
    expect(validateViewDoc(choice('x'))).toBeNull();
    expect(validateViewDoc(choice(''))).toBeNull();
    expect(issue(choice('nope')).message).toContain('options');
    expect(issue(choice('x', [{ value: 'x', label: 'Again' }])).code).toBe('duplicateId');
    expect(issue(choice('x', [{ value: 'y', label: ' ' }])).code).toBe('unlabelled');
    expect(validateViewDoc(doc(form([field('p', { path: { value: 'grant-1', folder: true } }), field('s', { secretRef: { value: null } })])))).toBeNull();
    expect(issue(doc(form([field('p', { path: { value: '', folder: false } })]))).code).toBe('invalid');
    expect(issue(doc(form([field('p', { weird: {} })]))).message).toContain('unknown kind');
  });

  it('checks review lists: states, ids and the state each finding names', () => {
    const list = (items: unknown[], states: unknown[]): NodeKind => ({ reviewList: { items, states } }) as unknown as NodeKind;
    const state = (id: string) => ({ id, label: id, tone: 'neutral', actions: [act()], needsReason: false });
    const item = (id: string, s: string) => ({ id, title: id, detail: '', location: '', tone: 'neutral', state: s, link: null });
    expect(validateViewDoc(doc(list([item('f', 'open')], [state('open')])))).toBeNull();
    expect(issue(doc(list([item('f', 'zz')], [state('open')]))).message).toContain('no state');
    expect(issue(doc(list([item('f', 'open'), item('f', 'open')], [state('open')]))).code).toBe('duplicateId');
    expect(issue(doc(list([], [state('open'), state('open')]))).code).toBe('duplicateId');
    expect(issue(doc(list([], [{ ...state('open'), actions: [act({ id: '' })] }]))).code).toBe('invalid');
    expect(issue(doc(list([], [{ ...state('open'), needsReason: 'yes' }]))).code).toBe('invalid');
  });

  it('checks matrices: headings, places and duplicates', () => {
    const m = (cells: unknown[]): NodeKind =>
      ({ matrix: { rows: [{ id: 'r', text: 'R' }], cols: [{ id: 'c', text: 'C' }], cells } }) as unknown as NodeKind;
    const cell = (row: number, col: number) => ({ row, col, text: 'x', tone: 'neutral', action: null });
    expect(validateViewDoc(doc(m([cell(0, 0)])))).toBeNull();
    expect(issue(doc(m([cell(1, 0)]))).message).toContain('outside');
    expect(issue(doc(m([cell(0, 1)]))).message).toContain('outside');
    expect(issue(doc(m([cell(0, 0), cell(0, 0)]))).message).toContain('same place');
    expect(issue(doc({ matrix: { rows: [{ id: 'r', text: 'R' }, { id: 'r', text: 'S' }], cols: [], cells: [] } })).code).toBe('duplicateId');
  });

  it('checks charts: one value per category, finite values and the point limit', () => {
    const chart = (series: unknown[], categories = ['a', 'b']): NodeKind => ({ chart: { kind: 'line', title: 'T', unit: '', categories, series } }) as unknown as NodeKind;
    expect(validateViewDoc(doc(chart([{ name: 'S', values: [1, 2] }])))).toBeNull();
    expect(issue(doc(chart([{ name: 'S', values: [1] }]))).message).toContain('1 values for 2 categories');
    expect(issue(doc(chart([{ name: 'S', values: [1, Number.NaN] }]))).message).toContain('finite');
    expect(issue(doc(chart([{ name: 'S', values: [1, 'x'] }]))).message).toContain('finite');
    expect(issue(doc(chart([{ name: ' ', values: [1, 2] }]))).code).toBe('unlabelled');
    const unit = (u: unknown): NodeKind => ({ chart: { kind: 'line', title: 'T', unit: u, categories: [], series: [] } }) as unknown as NodeKind;
    expect(validateViewDoc(doc(unit('%')))).toBeNull();
    expect(issue(doc(unit(5))).path).toContain('.unit');
    const cats = Array.from({ length: 2000 }, (_, i) => String(i));
    const wide = Array.from({ length: 11 }, (_, i) => ({ name: `s${i}`, values: cats.map(() => 1) }));
    expect(issue(doc(chart(wide, cats))).message).toContain('limit');
  });
});

describe('validateViewAction', () => {
  it('bounds the ids and the arguments of an action', () => {
    expect(validateViewAction({ view: 'v', action: 'a', args: { n: 1 } })).toBeNull();
    expect(validateViewAction({ view: '', action: 'a', args: null })?.code).toBe('invalid');
    expect(validateViewAction({ view: 'v', action: 'a'.repeat(129), args: null })?.code).toBe('invalid');
    expect(validateViewAction({ view: 'v', action: 'a', args: 'x'.repeat(VIEW_EXTRA_LIMITS.maxArgsBytes) })?.code).toBe('invalid');
    const cyc: Record<string, unknown> = {};
    cyc['self'] = cyc;
    expect(validateViewAction({ view: 'v', action: 'a', args: cyc })?.code).toBe('invalid');
    expect(validateViewAction({ view: 'v', action: 'a', args: undefined })).toBeNull();
  });
});
