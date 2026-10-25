import { VIEW_LIMITS, VIEW_SCHEMA, type ViewAction, type ViewDoc } from '../generated/view';

/** What was wrong with a document, with the path of the node that broke the rule (the TypeScript mirror of `paddock_view::ViewError`). */
export interface ViewIssue {
  readonly code: 'notADocument' | 'unsupportedSchema' | 'tooDeep' | 'tooManyNodes' | 'tooLarge' | 'stringTooLong' | 'duplicateId' | 'tooManyRows' | 'graphTooLarge' | 'badUrl' | 'unlabelled' | 'invalid';
  readonly path: string;
  /** A sentence for the user (the inline error shows it). */
  readonly message: string;
}

/** The limits of the engine that the mirror adds to {@link VIEW_LIMITS}: `MAX_ID`, `MAX_URL`, `MAX_ARGS`, `MAX_CHART_POINTS`, `MAX_SAFE_INT`. */
export const VIEW_EXTRA_LIMITS = { maxId: 128, maxUrl: 2048, maxArgsBytes: 64 * 1024, maxChartPoints: 20_000, maxSafeInt: Number.MAX_SAFE_INTEGER } as const;

type Rec = Record<string, unknown>;
const encoder = new TextEncoder();

class Stop extends Error {
  constructor(readonly issue: ViewIssue) {
    super(issue.message);
  }
}

const bytes = (s: string): number => (s.length * 3 <= VIEW_LIMITS.maxStringBytes ? s.length : encoder.encode(s).length);

function stop(code: ViewIssue['code'], path: string, message: string): never {
  throw new Stop({ code, path, message });
}
const invalid = (path: string, reason: string): never => stop('invalid', path, $localize`:@@pd.view.issue.invalid:${path}:path: is not valid: ${reason}:reason:.`);

const isRec = (v: unknown): v is Rec => v !== null && typeof v === 'object' && !Array.isArray(v);

/** Checks a received document (JSON from the engine) against the limits and rules of ADR 0002; the renderer shows only documents that pass. */
export function validateViewDoc(value: unknown): ViewIssue | null {
  try {
    new Checker().doc(value);
    return null;
  } catch (e) {
    if (e instanceof Stop) return e.issue;
    // Anything else is a bug of the mirror; a document that makes it throw is not shown.
    return { code: 'invalid', path: 'root', message: $localize`:@@pd.view.issue.unreadable:The view could not be read.` };
  }
}

/** Checks an action before it is sent: ids and arguments within limits (`validate_action`). */
export function validateViewAction(action: ViewAction): ViewIssue | null {
  try {
    const c = new Checker();
    c.id('view', action.view);
    c.id('action', action.action);
    c.args('args', action.args);
    return null;
  } catch (e) {
    return e instanceof Stop ? e.issue : { code: 'invalid', path: 'action', message: $localize`:@@pd.view.issue.unreadable:The view could not be read.` };
  }
}

/** Whitespace or a C0/C1 control character anywhere in the text (written as code-point tests, not a control-character regex). */
function hasSpaceOrControl(text: string): boolean {
  for (const ch of text) {
    const code = ch.codePointAt(0) ?? 0;
    if (/\s/.test(ch) || code <= 0x1f || (code >= 0x7f && code <= 0x9f)) return true;
  }
  return false;
}

/** The check of a `web_url`: `http` or `https`, a host, no user information, no spaces or control characters. */
export function isWebUrl(url: string): boolean {
  if (url.length > VIEW_EXTRA_LIMITS.maxUrl || hasSpaceOrControl(url)) return false;
  const m = /^https?:\/\/([^/?#]*)/i.exec(url);
  const authority = m?.[1] ?? '';
  return authority !== '' && !authority.includes('@') && !authority.startsWith(':');
}

class Checker {
  private nodes = 0;
  private readonly ids = new Set<string>();

  private count(path: string, n: number): void {
    this.nodes += n;
    if (this.nodes > VIEW_LIMITS.maxNodes) stop('tooManyNodes', path, $localize`:@@pd.view.issue.nodes:The view has more than ${VIEW_LIMITS.maxNodes}:max: items.`);
  }
  private rec(path: string, v: unknown): Rec {
    return isRec(v) ? v : invalid(path, $localize`:@@pd.view.issue.object:an object was expected`);
  }
  private list(path: string, v: unknown): readonly unknown[] {
    return Array.isArray(v) ? v : invalid(path, $localize`:@@pd.view.issue.list:a list was expected`);
  }
  private str(path: string, v: unknown): string {
    return typeof v === 'string' ? v : invalid(path, $localize`:@@pd.view.issue.string:text was expected`);
  }
  private num(path: string, v: unknown): number {
    return typeof v === 'number' && Number.isFinite(v) ? v : invalid(path, $localize`:@@pd.view.issue.number:a number was expected`);
  }
  private safeInt(path: string, v: unknown): number {
    const n = this.num(path, v);
    if (!Number.isInteger(n) || n < 0 || n > VIEW_EXTRA_LIMITS.maxSafeInt) invalid(path, $localize`:@@pd.view.issue.int:a whole number from 0 to 2^53 - 1 was expected`);
    return n;
  }
  private bool(path: string, v: unknown): boolean {
    return typeof v === 'boolean' ? v : invalid(path, $localize`:@@pd.view.issue.bool:true or false was expected`);
  }
  /** A string within the limit (may be empty). */
  private text(path: string, v: unknown): string {
    const s = this.str(path, v);
    if (bytes(s) > VIEW_LIMITS.maxStringBytes) stop('stringTooLong', path, $localize`:@@pd.view.issue.long:${path}:path: is longer than ${VIEW_LIMITS.maxStringBytes}:max: bytes.`);
    return s;
  }
  /** A string within the limit that shows something. */
  private label(path: string, v: unknown): string {
    const s = this.text(path, v);
    if (s.trim() === '') stop('unlabelled', path, $localize`:@@pd.view.issue.unlabelled:${path}:path: has no text, so nobody could read it.`);
    return s;
  }
  id(path: string, v: unknown): string {
    const s = this.str(path, v);
    if (s === '' || bytes(s) > VIEW_EXTRA_LIMITS.maxId) invalid(path, $localize`:@@pd.view.issue.id:an id is 1 to ${VIEW_EXTRA_LIMITS.maxId}:max: bytes`);
    return s;
  }
  private unique(path: string, seen: Set<string>, id: string): void {
    if (seen.has(id)) stop('duplicateId', path, $localize`:@@pd.view.issue.dup:${path}:path: repeats the id "${id}:id:".`);
    seen.add(id);
  }
  args(path: string, args: unknown): void {
    let size = Infinity;
    try {
      size = encoder.encode(JSON.stringify(args ?? null)).length;
    } catch {
      /* not serialisable: too large to send */
    }
    if (size > VIEW_EXTRA_LIMITS.maxArgsBytes) invalid(path, $localize`:@@pd.view.issue.args:arguments larger than ${VIEW_EXTRA_LIMITS.maxArgsBytes}:max: bytes`);
  }

  doc(value: unknown): void {
    if (!isRec(value)) stop('notADocument', 'root', $localize`:@@pd.view.issue.not-doc:The view is not a document.`);
    const d = value as Rec;
    if (d['schema'] !== VIEW_SCHEMA) {
      stop('unsupportedSchema', 'schema', $localize`:@@pd.view.issue.schema:This view needs a different version of the app (it is version ${String(d['schema'])}:found:, this app reads ${VIEW_SCHEMA}:supported:). Update the app to view this.`);
    }
    this.safeInt('etag', d['etag']);
    this.label('title', d['title']);
    this.node(d['root'], 'root', 1);
    // Last, like the engine: the cheap checks above bound what is left to count.
    const size = encoder.encode(JSON.stringify(d)).length;
    if (size > VIEW_LIMITS.maxEncodedBytes) stop('tooLarge', 'root', $localize`:@@pd.view.issue.large:The view is larger than ${VIEW_LIMITS.maxEncodedBytes}:max: bytes.`);
  }

  private action(path: string, v: unknown): void {
    const a = this.rec(path, v);
    this.id(`${path}.id`, a['id']);
    this.label(`${path}.label`, a['label']);
    if (a['confirm'] !== null && a['confirm'] !== undefined) this.label(`${path}.confirm`, a['confirm']);
    this.args(`${path}.args`, a['args']);
  }
  private optAction(path: string, v: unknown): void {
    if (v !== null && v !== undefined) this.action(path, v);
  }
  private link(path: string, v: unknown): void {
    const l = this.rec(path, v);
    if ('action' in l) return this.action(path, l['action']);
    const o = this.rec(path, l['openInBrowser']);
    this.label(`${path}.label`, o['label']);
    const url = this.str(`${path}.url`, o['url']);
    if (url.length > VIEW_EXTRA_LIMITS.maxUrl) stop('stringTooLong', `${path}.url`, $localize`:@@pd.view.issue.long-url:${path}:path: is a web address that is too long.`);
    if (!isWebUrl(url)) stop('badUrl', `${path}.url`, $localize`:@@pd.view.issue.url:${path}:path: is not a web address (http or https, without a user name).`);
  }
  private optLink(path: string, v: unknown): void {
    if (v !== null && v !== undefined) this.link(path, v);
  }

  private node(value: unknown, path: string, depth: number): void {
    if (depth > VIEW_LIMITS.maxDepth) stop('tooDeep', path, $localize`:@@pd.view.issue.deep:${path}:path: is nested more than ${VIEW_LIMITS.maxDepth}:max: levels deep.`);
    const n = this.rec(path, value);
    this.count(path, 1);
    const id = this.str(`${path}.id`, n['id']);
    if (id !== '') {
      this.id(`${path}.id`, id);
      this.unique(path, this.ids, id);
    }
    const kind = this.rec(`${path}.kind`, n['kind']);
    const tag = Object.keys(kind)[0] ?? '';
    const v = kind[tag];
    switch (tag) {
      case 'stack': {
        const s = this.rec(path, v);
        this.list(`${path}.children`, s['children']).forEach((c, i) => this.node(c, `${path}.children[${i}]`, depth + 1));
        return;
      }
      case 'text':
        this.text(`${path}.text`, this.rec(path, v)['text']);
        return;
      case 'table':
        return this.table(this.rec(path, v), path);
      case 'tree':
        return this.tree(this.rec(path, v), path, depth);
      case 'graph':
        return this.graph(this.rec(path, v), path);
      case 'diagram':
        return this.diagram(this.rec(path, v), path);
      case 'form':
        return this.form(this.rec(path, v), path);
      case 'reviewList':
        return this.review(this.rec(path, v), path);
      case 'matrix':
        return this.matrix(this.rec(path, v), path);
      case 'chart':
        return this.chart(this.rec(path, v), path);
      case 'badge':
        this.label(`${path}.text`, this.rec(path, v)['text']);
        return;
      case 'progress': {
        const p = this.rec(path, v);
        const value = this.num(`${path}.value`, p['value']);
        if (value < 0 || value > 1) invalid(`${path}.value`, $localize`:@@pd.view.issue.progress:progress is a number from 0 to 1`);
        this.text(`${path}.label`, p['label']);
        return;
      }
      case 'empty': {
        const e = this.rec(path, v);
        this.label(`${path}.title`, e['title']);
        this.text(`${path}.hint`, e['hint']);
        this.optAction(`${path}.action`, e['action']);
        return;
      }
      case 'needsAi': {
        const a = this.rec(path, v);
        this.label(`${path}.feature`, a['feature']);
        this.text(`${path}.reason`, a['reason']);
        this.action(`${path}.setup`, a['setup']);
        return;
      }
      case 'link': {
        const l = this.rec(path, v);
        this.label(`${path}.label`, l['label']);
        this.link(`${path}.target`, l['target']);
        return;
      }
      default:
        // A kind of a newer schema minor: counted as one node, shown as "update the app" for that node only.
        return;
    }
  }

  private cells(path: string, rows: readonly unknown[], columns: number): void {
    this.count(path, rows.length);
    rows.forEach((row, r) => {
      const cells = this.list(`${path}[${r}]`, row);
      if (cells.length !== columns) invalid(`${path}[${r}]`, $localize`:@@pd.view.issue.cells:${cells.length}:n: cells for ${columns}:columns: columns`);
      cells.forEach((cell, c) => {
        const p = `${path}[${r}][${c}]`;
        const cr = this.rec(p, cell);
        this.text(p, cr['text']);
        this.optLink(`${p}.link`, cr['link']);
      });
    });
  }

  private table(t: Rec, path: string): void {
    const keys = new Set<string>();
    const columns = this.list(`${path}.columns`, t['columns']);
    columns.forEach((col, i) => {
      const p = `${path}.columns[${i}]`;
      const c = this.rec(p, col);
      this.unique(p, keys, this.id(`${p}.key`, c['key']));
      this.label(`${p}.label`, c['label']);
      this.text(`${p}.unit`, c['unit']);
      if (c['align'] !== 'start' && c['align'] !== 'end') invalid(`${p}.align`, $localize`:@@pd.view.issue.align:"start" or "end" was expected`);
    });
    if (t['sort'] !== null && t['sort'] !== undefined) {
      const col = this.str(`${path}.sort`, this.rec(`${path}.sort`, t['sort'])['column']);
      if (!keys.has(col)) invalid(`${path}.sort`, $localize`:@@pd.view.issue.sort:there is no column "${col}:column:"`);
    }
    this.optAction(`${path}.rowAction`, t['rowAction']);
    const p = `${path}.rows`;
    const rows = this.rec(p, t['rows']);
    if ('inline' in rows) {
      const list = this.list(p, rows['inline']);
      if (list.length > VIEW_LIMITS.maxInlineRows) stop('tooManyRows', p, $localize`:@@pd.view.issue.rows:${p}:path: has ${list.length}:rows: rows; more than ${VIEW_LIMITS.maxInlineRows}:max: must be paged.`);
      return this.cells(p, list, columns.length);
    }
    const g = this.rec(p, rows['paged']);
    const total = this.safeInt(`${p}.total`, g['total']);
    const pageSize = this.safeInt(`${p}.pageSize`, g['pageSize']);
    const page = this.safeInt(`${p}.page`, g['page']);
    const shown = this.list(p, g['rows']);
    if (pageSize === 0 || pageSize > VIEW_LIMITS.maxInlineRows) invalid(p, $localize`:@@pd.view.issue.page:a page is 1 to ${VIEW_LIMITS.maxInlineRows}:max: rows`);
    if (shown.length > pageSize) stop('tooManyRows', p, $localize`:@@pd.view.issue.rows:${p}:path: has ${shown.length}:rows: rows; more than ${pageSize}:max: must be paged.`);
    const first = page * pageSize;
    if (first > 0 && first >= total) invalid(p, $localize`:@@pd.view.issue.beyond:the page is beyond the last row`);
    if (shown.length > Math.max(0, total - first)) invalid(p, $localize`:@@pd.view.issue.more-rows:more rows on the page than the total allows`);
    this.cells(p, shown, columns.length);
  }

  private tree(t: Rec, path: string, depth: number): void {
    const nodes = this.list(`${path}.nodes`, t['nodes']);
    const lazy = this.bool(`${path}.lazy`, t['lazy']);
    this.count(path, nodes.length);
    const seen = new Set<string>();
    let prev: number | null = null;
    nodes.forEach((raw, i) => {
      const p = `${path}.nodes[${i}]`;
      const n = this.rec(p, raw);
      this.unique(p, seen, this.id(`${p}.id`, n['id']));
      this.label(`${p}.label`, n['label']);
      const d = this.safeInt(`${p}.depth`, n['depth']);
      if (d > (prev === null ? 0 : prev + 1)) invalid(p, $localize`:@@pd.view.issue.tree-depth:a tree node is at most one level below the node before it`);
      if (depth + d + 1 > VIEW_LIMITS.maxDepth) stop('tooDeep', p, $localize`:@@pd.view.issue.deep:${p}:path: is nested more than ${VIEW_LIMITS.maxDepth}:max: levels deep.`);
      if (this.bool(`${p}.expandable`, n['expandable']) && !lazy) invalid(p, $localize`:@@pd.view.issue.tree-lazy:only a lazy tree has expandable nodes`);
      this.optAction(`${p}.action`, n['action']);
      prev = d;
    });
  }

  private shape(path: string, nodes: number, edges: number): void {
    if (nodes > VIEW_LIMITS.maxGraphNodes || edges > VIEW_LIMITS.maxGraphEdges) {
      stop('graphTooLarge', path, $localize`:@@pd.view.issue.graph:${path}:path: has ${nodes}:nodes: nodes and ${edges}:edges: edges; the limit is ${VIEW_LIMITS.maxGraphNodes}:maxn: and ${VIEW_LIMITS.maxGraphEdges}:maxe:.`);
    }
  }
  private edges(path: string, ids: ReadonlySet<string>, edges: readonly unknown[]): void {
    edges.forEach((raw, i) => {
      const p = `${path}[${i}]`;
      const e = this.rec(p, raw);
      this.text(`${p}.label`, e['label']);
      for (const end of ['from', 'to']) {
        const id = this.str(`${p}.${end}`, e[end]);
        if (!ids.has(id)) invalid(`${p}.${end}`, $localize`:@@pd.view.issue.no-node:there is no node "${id}:id:"`);
      }
    });
  }

  private graph(g: Rec, path: string): void {
    const nodes = this.list(`${path}.nodes`, g['nodes']);
    const edges = this.list(`${path}.edges`, g['edges']);
    this.shape(path, nodes.length, edges.length);
    this.count(path, nodes.length);
    const seen = new Set<string>();
    nodes.forEach((raw, i) => {
      const p = `${path}.nodes[${i}]`;
      const n = this.rec(p, raw);
      this.unique(p, seen, this.id(`${p}.id`, n['id']));
      this.label(`${p}.label`, n['label']);
      this.optAction(`${p}.action`, n['action']);
    });
    this.edges(`${path}.edges`, seen, edges);
  }

  private diagram(d: Rec, path: string): void {
    const m = this.rec(`${path}.model`, d['model']);
    const elements = this.list(`${path}.elements`, m['elements']);
    const relations = this.list(`${path}.relations`, m['relations']);
    this.shape(path, elements.length, relations.length);
    this.count(path, elements.length);
    const seen = new Set<string>();
    elements.forEach((raw, i) => {
      const p = `${path}.elements[${i}]`;
      const e = this.rec(p, raw);
      this.unique(p, seen, this.id(`${p}.id`, e['id']));
      this.label(`${p}.label`, e['label']);
      this.text(`${p}.detail`, e['detail']);
      this.text(`${p}.group`, e['group']);
    });
    this.edges(`${path}.relations`, seen, relations);
  }

  private form(f: Rec, path: string): void {
    const fields = this.list(`${path}.fields`, f['fields']);
    this.count(path, fields.length);
    const seen = new Set<string>();
    fields.forEach((raw, i) => {
      const p = `${path}.fields[${i}]`;
      const field = this.rec(p, raw);
      this.unique(p, seen, this.id(`${p}.id`, field['id']));
      this.label(`${p}.label`, field['label']);
      this.text(`${p}.help`, field['help']);
      this.fieldKind(this.rec(`${p}.kind`, field['kind']), `${p}.kind`);
    });
    this.action(`${path}.submit`, f['submit']);
  }

  private fieldKind(kind: Rec, p: string): void {
    const tag = Object.keys(kind)[0] ?? '';
    const v = kind[tag];
    switch (tag) {
      case 'text': {
        const t = this.rec(p, v);
        this.text(`${p}.value`, t['value']);
        this.text(`${p}.placeholder`, t['placeholder']);
        return;
      }
      case 'number': {
        const n = this.rec(p, v);
        const value = this.num(`${p}.value`, n['value']);
        const lo = n['min'] === null || n['min'] === undefined ? null : this.num(`${p}.min`, n['min']);
        const hi = n['max'] === null || n['max'] === undefined ? null : this.num(`${p}.max`, n['max']);
        if ((lo !== null && hi !== null && lo > hi) || (lo !== null && value < lo) || (hi !== null && value > hi)) invalid(p, $localize`:@@pd.view.issue.minmax:the value is outside min and max`);
        return;
      }
      case 'choice': {
        const c = this.rec(p, v);
        const seen = new Set<string>();
        this.list(`${p}.options`, c['options']).forEach((raw, i) => {
          const op = `${p}.options[${i}]`;
          const o = this.rec(op, raw);
          this.unique(op, seen, this.text(`${op}.value`, o['value']));
          this.label(`${op}.label`, o['label']);
        });
        const value = this.text(`${p}.value`, c['value']);
        if (value !== '' && !seen.has(value)) invalid(`${p}.value`, $localize`:@@pd.view.issue.option:not one of the options`);
        return;
      }
      case 'toggle':
        this.bool(`${p}.value`, this.rec(p, v)['value']);
        return;
      case 'path':
      case 'secretRef': {
        const value = this.rec(p, v)['value'];
        if (value !== null && value !== undefined) this.id(`${p}.value`, value);
        return;
      }
      default:
        invalid(p, $localize`:@@pd.view.issue.field:unknown kind of field`);
    }
  }

  private review(r: Rec, path: string): void {
    const items = this.list(`${path}.items`, r['items']);
    this.count(path, items.length);
    const states = new Set<string>();
    this.list(`${path}.states`, r['states']).forEach((raw, i) => {
      const p = `${path}.states[${i}]`;
      const s = this.rec(p, raw);
      this.unique(p, states, this.id(`${p}.id`, s['id']));
      this.label(`${p}.label`, s['label']);
      this.bool(`${p}.needsReason`, s['needsReason']);
      this.list(`${p}.actions`, s['actions']).forEach((a, k) => this.action(`${p}.actions[${k}]`, a));
    });
    const seen = new Set<string>();
    items.forEach((raw, i) => {
      const p = `${path}.items[${i}]`;
      const f = this.rec(p, raw);
      this.unique(p, seen, this.id(`${p}.id`, f['id']));
      this.label(`${p}.title`, f['title']);
      this.text(`${p}.detail`, f['detail']);
      this.text(`${p}.location`, f['location']);
      const state = this.str(`${p}.state`, f['state']);
      if (!states.has(state)) invalid(`${p}.state`, $localize`:@@pd.view.issue.state:there is no state "${state}:state:"`);
      this.optLink(`${p}.link`, f['link']);
    });
  }

  private matrix(m: Rec, path: string): void {
    const cells = this.list(`${path}.cells`, m['cells']);
    this.count(path, cells.length);
    const sizes: Record<string, number> = {};
    for (const name of ['rows', 'cols']) {
      const seen = new Set<string>();
      const labels = this.list(`${path}.${name}`, m[name]);
      sizes[name] = labels.length;
      labels.forEach((raw, i) => {
        const p = `${path}.${name}[${i}]`;
        const l = this.rec(p, raw);
        this.unique(p, seen, this.id(`${p}.id`, l['id']));
        this.label(`${p}.text`, l['text']);
      });
    }
    const at = new Set<string>();
    cells.forEach((raw, i) => {
      const p = `${path}.cells[${i}]`;
      const c = this.rec(p, raw);
      const row = this.safeInt(`${p}.row`, c['row']);
      const col = this.safeInt(`${p}.col`, c['col']);
      if (row >= (sizes['rows'] ?? 0) || col >= (sizes['cols'] ?? 0)) invalid(p, $localize`:@@pd.view.issue.outside:the cell is outside the matrix`);
      if (at.has(`${row}:${col}`)) invalid(p, $localize`:@@pd.view.issue.same-place:two cells at the same place`);
      at.add(`${row}:${col}`);
      this.text(`${p}.text`, c['text']);
      this.optAction(`${p}.action`, c['action']);
    });
  }

  private chart(c: Rec, path: string): void {
    this.text(`${path}.title`, c['title']);
    this.text(`${path}.unit`, c['unit']);
    const categories = this.list(`${path}.categories`, c['categories']);
    categories.forEach((cat, i) => this.text(`${path}.categories[${i}]`, cat));
    const series = this.list(`${path}.series`, c['series']);
    const points = series.reduce<number>((sum, s) => sum + (isRec(s) && Array.isArray(s['values']) ? s['values'].length : 0), 0);
    if (points > VIEW_EXTRA_LIMITS.maxChartPoints) invalid(path, $localize`:@@pd.view.issue.points:${points}:n: values; the limit is ${VIEW_EXTRA_LIMITS.maxChartPoints}:max:`);
    this.count(path, series.length);
    series.forEach((raw, i) => {
      const p = `${path}.series[${i}]`;
      const s = this.rec(p, raw);
      this.label(`${p}.name`, s['name']);
      const values = this.list(`${p}.values`, s['values']);
      if (values.length !== categories.length) invalid(p, $localize`:@@pd.view.issue.series:${values.length}:n: values for ${categories.length}:c: categories`);
      if (values.some((v) => typeof v !== 'number' || !Number.isFinite(v))) invalid(p, $localize`:@@pd.view.issue.finite:values are finite numbers`);
    });
  }
}

/** Narrows a validated value to a document. */
export const asViewDoc = (value: unknown): ViewDoc => value as ViewDoc;
