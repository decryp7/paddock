import type { ActionRef, Cell, Node, NodeKind, Tone, ViewDoc } from '../generated/view';

const act = (id: string, label: string, extra: Partial<ActionRef> = {}): ActionRef => ({ id, label, confirm: null, args: null, writes: false, ...extra });
const node = (id: string, kind: NodeKind): Node => ({ id, kind });
const cell = (text: string, tone: Tone = 'neutral', link: Cell['link'] = null): Cell => ({ text, tone, link });

/**
 * A sample document with one node of every kind (and one kind this renderer does not know), for the gallery, the specs and the
 * contact sheet. It passes `validateViewDoc`; changing it changes what the review sheet shows.
 */
export function sampleViewDoc(etag = 1): ViewDoc {
  return {
    schema: 1,
    title: 'Release readiness',
    etag,
    root: node('root', {
      stack: {
        dir: 'column',
        children: [
          node('h', { text: { text: 'Release 1.4: what is left', tone: 'neutral', style: 'heading' } }),
          node('lead', { text: { text: 'Three checks need a decision before the tag. The numbers are from the last full run.', tone: 'neutral', style: 'body' } }),
          node('code', { text: { text: 'cargo test -p paddock-view --lib', tone: 'neutral', style: 'code' } }),
          node('cap', { text: { text: 'Updated a minute ago', tone: 'neutral', style: 'caption' } }),
          node('badges', {
            stack: {
              dir: 'row',
              children: [
                node('b1', { badge: { text: 'Neutral', tone: 'neutral' } }),
                node('b2', { badge: { text: 'Info', tone: 'info' } }),
                node('b3', { badge: { text: 'Passing', tone: 'good' } }),
                node('b4', { badge: { text: 'Flaky', tone: 'warn' } }),
                node('b5', { badge: { text: 'Failing', tone: 'bad' } }),
              ],
            },
          }),
          node('prog', { progress: { value: 0.62, label: 'Checks passed' } }),
          node('t1', {
            table: {
              columns: [
                { key: 'name', label: 'Check', sortable: true },
                { key: 'state', label: 'State', sortable: true },
                { key: 'ms', label: 'Time (ms)', sortable: true },
                { key: 'log', label: 'Log', sortable: false },
              ],
              rows: {
                inline: [
                  [cell('Unit tests'), cell('Passing', 'good'), cell('1,240'), cell('log', 'neutral', { action: act('open-log', 'Open the log') })],
                  [cell('Integration tests'), cell('Flaky', 'warn'), cell('8,911'), cell('log', 'neutral', { action: act('open-log', 'Open the log') })],
                  [cell('Lint'), cell('Failing', 'bad'), cell('530'), cell('docs', 'neutral', { openInBrowser: { url: 'https://example.com/lint', label: 'Open the lint guide' } })],
                ],
              },
              rowAction: act('open-check', 'Open'),
              sort: { column: 'name', descending: false },
            },
          }),
          node('t2', {
            table: {
              columns: [
                { key: 'file', label: 'File', sortable: true },
                { key: 'lines', label: 'Lines', sortable: false },
              ],
              rows: { paged: { total: 120, pageSize: 2, page: 1, rows: [[cell('src/model.rs'), cell('652')], [cell('src/validate.rs'), cell('583')]] } },
              rowAction: null,
              sort: null,
            },
          }),
          node('tree', {
            tree: {
              lazy: true,
              nodes: [
                { id: 'crates', label: 'crates', depth: 0, tone: 'neutral', icon: 'folder', expandable: true, action: null },
                { id: 'view', label: 'paddock-view', depth: 1, tone: 'good', icon: 'folder', expandable: true, action: null },
                { id: 'model', label: 'model.rs', depth: 2, tone: 'neutral', icon: 'file', expandable: false, action: act('open-file', 'Open file') },
                { id: 'ui', label: 'ui', depth: 0, tone: 'neutral', icon: 'folder', expandable: true, action: null },
              ],
            },
          }),
          node('g1', {
            graph: {
              layout: 'layered',
              nodes: [
                { id: 'a', label: 'Build', tone: 'good', action: null },
                { id: 'b', label: 'Unit tests', tone: 'good', action: act('open-node', 'Open step') },
                { id: 'c', label: 'Integration tests', tone: 'warn', action: act('open-node', 'Open step') },
                { id: 'd', label: 'Package', tone: 'neutral', action: null },
                { id: 'e', label: 'Publish', tone: 'bad', action: null },
              ],
              edges: [
                { from: 'a', to: 'b', label: '' },
                { from: 'a', to: 'c', label: 'needs db' },
                { from: 'b', to: 'd', label: '' },
                { from: 'c', to: 'd', label: '' },
                { from: 'd', to: 'e', label: '' },
                { from: 'e', to: 'a', label: 'retry' },
              ],
            },
          }),
          node('g2', {
            graph: {
              layout: 'radial',
              nodes: [
                { id: 'hub', label: 'paddock-ipc', tone: 'info', action: null },
                { id: 'p1', label: 'paddock-view', tone: 'neutral', action: null },
                { id: 'p2', label: 'paddock-shell', tone: 'neutral', action: null },
                { id: 'p3', label: 'paddock-env', tone: 'neutral', action: null },
              ],
              edges: [
                { from: 'p1', to: 'hub', label: '' },
                { from: 'p2', to: 'hub', label: '' },
                { from: 'p3', to: 'hub', label: '' },
              ],
            },
          }),
          node('d1', {
            diagram: {
              kind: 'class',
              model: {
                elements: [
                  { id: 'doc', label: 'ViewDoc', detail: 'schema: u16\ntitle: String\netag: u64', group: 'model' },
                  { id: 'nd', label: 'Node', detail: 'id: String\nkind: NodeKind', group: 'model' },
                ],
                relations: [{ from: 'doc', to: 'nd', label: 'root' }],
              },
            },
          }),
          node('d2', {
            diagram: {
              kind: 'sequence',
              model: {
                elements: [
                  { id: 'ui', label: 'Window', detail: '', group: '' },
                  { id: 'host', label: 'Host', detail: '', group: '' },
                  { id: 'plugin', label: 'Plugin', detail: '', group: '' },
                ],
                relations: [
                  { from: 'ui', to: 'host', label: 'ViewAction' },
                  { from: 'host', to: 'plugin', label: 'action' },
                  { from: 'plugin', to: 'plugin', label: 'work' },
                  { from: 'plugin', to: 'host', label: 'ViewPatch' },
                  { from: 'host', to: 'ui', label: 'replace' },
                ],
              },
            },
          }),
          node('form', {
            form: {
              fields: [
                { id: 'name', label: 'Release name', help: 'Shown in the changelog.', required: true, kind: { text: { value: 'v1.4', placeholder: 'v1.4', multiline: false } } },
                { id: 'notes', label: 'Notes', help: '', required: false, kind: { text: { value: '', placeholder: 'What changed', multiline: true } } },
                { id: 'retries', label: 'Retries', help: '0 to 5', required: false, kind: { number: { value: 2, min: 0, max: 5 } } },
                { id: 'channel', label: 'Channel', help: '', required: true, kind: { choice: { value: 'beta', options: [{ value: 'beta', label: 'Beta' }, { value: 'stable', label: 'Stable' }] } } },
                { id: 'sign', label: 'Sign the bundle', help: 'Uses the ad-hoc identity.', required: false, kind: { toggle: { value: true } } },
                { id: 'out', label: 'Output folder', help: '', required: false, kind: { path: { value: null, folder: true } } },
                { id: 'token', label: 'Publish token', help: 'A stored secret; the view never shows it.', required: false, kind: { secretRef: { value: null } } },
              ],
              submit: act('publish', 'Publish', { writes: true, confirm: 'Publish release v1.4?' }),
            },
          }),
          node('review', {
            reviewList: {
              states: [
                { id: 'open', label: 'Open', tone: 'warn', actions: [act('accept', 'Accept', { writes: true }), act('dismiss', 'Dismiss', { writes: true })], needsReason: false },
                { id: 'suppressed', label: 'Suppressed', tone: 'neutral', actions: [act('restore', 'Restore', { writes: true })], needsReason: false },
                { id: 'blocked', label: 'Blocked', tone: 'bad', actions: [act('suppress', 'Suppress', { writes: true })], needsReason: true },
              ],
              items: [
                { id: 'f1', title: 'Unchecked unwrap on user data', detail: 'The parser calls unwrap on a value the file controls.', location: 'src/codec.rs:42', tone: 'warn', state: 'open', link: { openInBrowser: { url: 'https://example.com/rule/unwrap', label: 'Read the rule' } } },
                { id: 'f2', title: 'Missing doc comment', detail: '', location: 'src/lib.rs:10', tone: 'info', state: 'suppressed', link: null },
                { id: 'f3', title: 'Hard-coded version in a test', detail: 'Read the version from its single source.', location: 'tests/it/version.rs:7', tone: 'bad', state: 'blocked', link: null },
              ],
            },
          }),
          node('matrix', {
            matrix: {
              rows: [{ id: 'r1', text: 'REQ-1 Open a file' }, { id: 'r2', text: 'REQ-2 Save a file' }],
              cols: [{ id: 'c1', text: 'Unit' }, { id: 'c2', text: 'End to end' }],
              cells: [
                { row: 0, col: 0, text: 'Covered', tone: 'good', action: null },
                { row: 0, col: 1, text: 'Covered', tone: 'good', action: act('open-case', 'Open case') },
                { row: 1, col: 1, text: 'Missing', tone: 'bad', action: act('open-case', 'Open case') },
              ],
            },
          }),
          node('c1', { chart: { kind: 'line', title: 'Test time per run', categories: ['Mon', 'Tue', 'Wed', 'Thu'], series: [{ name: 'Seconds', values: [41, 44, 39, 52] }] } }),
          node('c2', { chart: { kind: 'bar', title: 'Failures by area', categories: ['core', 'ui', 'ci'], series: [{ name: 'Failures', values: [3, 1, 4] }] } }),
          node('c3', { chart: { kind: 'area', title: 'Open findings', categories: ['W1', 'W2', 'W3'], series: [{ name: 'Open', values: [12, 8, 5] }] } }),
          node('c4', { chart: { kind: 'pie', title: 'Findings by state', categories: ['Open', 'Suppressed', 'Blocked'], series: [{ name: 'Findings', values: [5, 2, 1] }] } }),
          node('c5', { chart: { kind: 'scatter', title: 'Size against time', categories: ['a', 'b', 'c', 'd'], series: [{ name: 'Runs', values: [3, 7, 4, 9] }] } }),
          node('empty', { empty: { icon: 'search', title: 'Nothing matches', hint: 'Change the filter or run the checks again.', action: act('rerun', 'Run the checks') } }),
          node('ai', { needsAi: { feature: 'Explain the failing check', reason: 'Pitwall is not running.', setup: act('setup-ai', 'Set up Pitwall') } }),
          node('l1', { link: { label: 'Open the run', target: { action: act('open-run', 'Open the run') } } }),
          node('l2', { link: { label: 'Release guide', target: { openInBrowser: { url: 'https://example.com/guide', label: 'Release guide' } } } }),
          { id: 'future', kind: { hologram: { depth: 3 } } as unknown as NodeKind },
        ],
      },
    }),
  };
}
