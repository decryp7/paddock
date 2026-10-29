import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { form, FormField, maxLength, min, minLength, required, validate } from '@angular/forms/signals';
import { PdChart, PdSunburst, type PdChartSpec, type PdSunburstNode } from '@decryp7/paddock-ui/chart';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { PdCell, PdTable, PdTableEmpty, type PdColumn, type PdRowAction, type PdRowActionEvent, type PdSort } from '@decryp7/paddock-ui/table';

interface Plugin {
  id: string;
  name: string;
  kind: string;
  version: string;
  size: number;
  path: string;
  state: 'ready' | 'failed' | 'disabled';
}

const KINDS = ['Format', 'Viewer', 'Exporter', 'Importer'];
const PLUGINS: Plugin[] = Array.from({ length: 14 }, (_, i) => ({
  id: `p${i}`,
  name: ['Syslog', 'Journal', 'Trace viewer', 'CSV export', 'Crash dump', 'Event log', 'Timeline', 'JSON lines', 'Diff view', 'Archive', 'Heap map', 'Net capture', 'Profile', 'Report'][i] as string,
  kind: KINDS[i % KINDS.length] as string,
  version: `1.${i % 5}.${i % 3}`,
  size: 120 + ((i * 7919) % 900),
  path: `/Users/alex/Library/Application Support/Debrief/plugins/${['syslog', 'journal', 'trace', 'csv', 'dump', 'events', 'timeline', 'jsonl', 'diff', 'archive', 'heap', 'net', 'profile', 'report'][i]}/plugin-${i}.bin`,
  state: i === 3 ? 'failed' : i === 9 ? 'disabled' : 'ready',
}));
const MANY: Plugin[] = Array.from({ length: 5000 }, (_, i) => ({
  id: `m${i}`,
  name: `Record ${String(i + 1).padStart(4, '0')}`,
  kind: KINDS[i % KINDS.length] as string,
  version: `2.${i % 9}.0`,
  size: ((i + 1) * 37) % 4096,
  path: `/data/records/${i + 1}/entry.bin`,
  state: 'ready',
}));

const COLUMNS: PdColumn<Plugin>[] = [
  { key: 'name', header: 'Name', value: (p) => p.name, required: true, width: 'minmax(0, 2fr)' },
  { key: 'kind', header: 'Kind', value: (p) => p.kind, hideBelow: 420, width: '96px' },
  { key: 'version', header: 'Version', value: (p) => p.version, hideBelow: 560, width: '80px' },
  { key: 'size', header: 'Size (KiB)', value: (p) => p.size, align: 'end', width: '96px' },
  { key: 'state', header: 'State', value: (p) => p.state, hidden: true, width: '96px' },
  { key: 'path', header: 'Location', value: (p) => p.path, truncate: 'middle', hideBelow: 760, width: 'minmax(0, 3fr)' },
];

/** The data parts of paddock-ui (plan X21): table, chart and forms. */
@Component({
  selector: 'gal-data',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdTable, PdCell, PdTableEmpty, PdChart, PdSunburst, PdField, PdFieldControl, FormField],
  template: `
    <section class="gal-section" data-testid="table" aria-labelledby="h-table">
      <h2 id="h-table">Data table</h2>
      <p>Sort by a header, filter, choose columns; arrows, Home, End and Enter on a row; Shift+F10 or a right-click opens the row menu. Columns drop out as the table narrows.</p>
      <div class="gal-box gal-pad" data-testid="table-small">
        <pd-table
          label="Plugins"
          [rows]="plugins"
          [columns]="columns"
          [rowKey]="key"
          [rowActions]="actions"
          [(sort)]="sort"
          [(selectedKey)]="selected"
          (rowActivate)="opened.set($event.name)"
          (action)="chosen.set(describe($event))"
        >
          <ng-template pdCell="state" let-row><span class="gal-state" [attr.data-state]="row.state">{{ row.state }}</span></ng-template>
        </pd-table>
      </div>
      <p data-testid="table-status">Opened: {{ opened() || 'none' }}. Chose: {{ chosen() || 'nothing' }}.</p>
      <h3>States</h3>
      <div class="gal-states-grid">
        <div class="gal-box gal-pad" data-testid="box-selected">
          <pd-table label="Selected and sorted" [rows]="plugins.slice(0, 5)" [columns]="columns" [rowKey]="key" [rowActions]="actions" [filterable]="false" selectedKey="p1" [sort]="{ key: 'size', direction: 'desc' }" />
        </div>
        <div class="gal-box gal-pad" data-testid="box-empty">
          <pd-table label="Empty" [rows]="none" [columns]="columns" [rowKey]="key" [rowActions]="actions" [filterable]="false">
            <div pdTableEmpty class="gal-next"><strong>No plugins installed</strong><span>Plugins add file formats and viewers.</span><button class="pd-btn pd-btn--primary" type="button">Install a plugin</button></div>
          </pd-table>
        </div>
        <div class="gal-box gal-pad" data-testid="box-loading">
          <pd-table label="Loading" [rows]="none" [columns]="columns" [rowKey]="key" [rowActions]="actions" [filterable]="false" state="loading" />
        </div>
        <div class="gal-box gal-pad" data-testid="box-error">
          <pd-table label="Error" [rows]="none" [columns]="columns" [rowKey]="key" [rowActions]="actions" [filterable]="false" state="error" errorText="The plugin list could not be read." />
        </div>
      </div>
      <h3>Grouped, and 5,000 rows in a virtual viewport</h3>
      <div class="gal-box gal-pad" data-testid="table-virtual">
        <pd-table label="Records" [rows]="many" [columns]="columns" [rowKey]="key" groupBy="kind" [maxHeight]="260" [virtual]="true" />
      </div>
    </section>

    <section class="gal-section" data-testid="chart" aria-labelledby="h-pdchart">
      <h2 id="h-pdchart">Chart</h2>
      <p>The wrapper draws a spec in the family theme; each chart has a name and a text twin for screen readers.</p>
      <div class="gal-charts">
        <div class="gal-box"><pd-chart label="Request rate" [spec]="line" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Outcome per day, last 7 days" [spec]="stacked" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Latency p95, last 24 h" [spec]="area" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Disk use by kind" [spec]="donut" [height]="200" /></div>
        <div class="gal-box" data-testid="chart-loading-box"><pd-chart label="Latency p95, last 24 h" [spec]="area" [height]="200" state="loading" /></div>
        <div class="gal-box" data-testid="chart-error-box"><pd-chart label="Latency p95, last 24 h" [spec]="area" [height]="200" state="error" errorText="The latency data could not be loaded." /></div>
      </div>
    </section>

    <section class="gal-section" data-testid="sunburst" aria-labelledby="h-sunburst">
      <h2 id="h-sunburst">Sunburst</h2>
      <p>
        A drillable ring chart for hierarchies: click or press Enter on a folder to make it the centre, the centre goes up one level. Hidden space is hatched and not yet counted space is a dashed ghost,
        so status is never colour alone; the list beside the chart carries every value.
      </p>
      <div class="gal-sunburst">
        <div class="gal-box gal-pad" data-testid="sunburst-box">
          <pd-sunburst label="Disk usage of Macintosh HD" [root]="tree" [(centre)]="burstCentre" [format]="bytes" />
        </div>
        <div class="gal-box gal-pad">
          <ul class="gal-key" aria-label="Largest folders">
            @for (c of tree.children; track c.id; let i = $index) {
              <li>
                <span class="gal-swatch" [attr.data-kind]="c.kind ?? 'item'" [style.background]="c.kind ? null : 'var(--pd-chart-' + (i < 5 ? i + 1 : 8) + ')'"></span>
                <span>{{ c.label }}</span>
                <span class="gal-key-value">{{ bytes(c.value) }}</span>
              </li>
            }
          </ul>
          <p data-testid="sunburst-centre">Centre: {{ burstCentre() ?? 'root' }}</p>
        </div>
        <div class="gal-box gal-pad" data-testid="sunburst-loading-box"><pd-sunburst label="Disk usage, loading" [root]="null" state="loading" [format]="bytes" /></div>
        <div class="gal-box gal-pad" data-testid="sunburst-empty-box"><pd-sunburst label="Disk usage, empty" [root]="null" [format]="bytes" emptyTitle="Nothing scanned yet" emptyText="Scan a volume to see where the space went."><button pdSunburstEmptyAction type="button" class="pd-btn pd-btn--primary">Scan this volume</button></pd-sunburst></div>
        <div class="gal-box gal-pad" data-testid="sunburst-error-box"><pd-sunburst label="Disk usage, error" [root]="null" state="error" errorText="The scan stopped: the volume stopped answering." [format]="bytes" /></div>
      </div>
    </section>

    <section class="gal-section" data-testid="forms" aria-labelledby="h-forms">
      <h2 id="h-forms">Forms</h2>
      <p>Signal Forms with <code>pd-field</code>: the error appears after the field was left, or on submit.</p>
      <form class="gal-form gal-box" (submit)="submit($event)" novalidate>
        <pd-field label="Name" hint="Shown in the list" [state]="f.name()" [submitted]="submitted()">
          <input type="text" pdFieldControl [formField]="f.name" autocomplete="off" />
        </pd-field>
        <pd-field label="Port" hint="1 to 65535" size="short" [state]="f.port()" [submitted]="submitted()">
          <input type="number" pdFieldControl [formField]="f.port" />
        </pd-field>
        <pd-field label="Workspace" hint="Set by your administrator">
          <input type="text" pdFieldControl value="Debrief" disabled />
        </pd-field>
        <pd-field label="Level" hint="A native select in a field" data-testid="native-select-field">
          <select pdFieldControl><option>Standard</option><option>Compact</option></select>
        </pd-field>
        <pd-field label="Folder" hint="A picker with no text control" data-testid="plain-field">
          <div class="gal-row"><span>~/builds</span><button type="button" class="pd-btn">Choose folder</button></div>
        </pd-field>
        <div class="gal-row"><button class="pd-btn pd-btn--primary" type="submit">Save</button></div>
      </form>
      <p data-testid="forms-status">{{ saved() ? 'Saved ' + saved() : 'Not saved' }}</p>
    </section>
  `,
  styles: `
    .gal-charts { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr)); gap: var(--pd-space-3); }
    .gal-sunburst { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 24rem), 1fr)); gap: var(--pd-space-3); align-items: start; }
    .gal-key { list-style: none; margin: 0 0 var(--pd-space-3); padding: 0; display: grid; gap: var(--pd-space-2); }
    .gal-key li { display: grid; grid-template-columns: 16px 1fr auto; gap: var(--pd-space-2); align-items: center; }
    .gal-key-value { font-variant-numeric: tabular-nums; color: var(--pd-text-muted); }
    .gal-swatch { width: 12px; height: 12px; border-radius: 2px; background: var(--pd-chart-8); }
    .gal-swatch[data-kind='hidden'] { background: repeating-linear-gradient(45deg, var(--pd-text-subtle) 0 1.5px, var(--pd-surface-2) 1.5px 5px); }
    .gal-swatch[data-kind='pending'] { background: var(--pd-surface-1); border: 1px dashed var(--pd-border-strong); }
    .gal-form { display: grid; gap: var(--pd-space-3); padding: var(--pd-space-4); max-width: 28rem; }
    .gal-pad { padding: var(--pd-space-3); }
    .gal-states-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 36rem), 1fr)); gap: var(--pd-space-3); }
    .gal-next { display: grid; justify-items: center; gap: var(--pd-space-2); }
    .gal-next span { color: var(--pd-text-muted); }
    .gal-state { color: var(--pd-text-muted); }
    .gal-state[data-state='failed'] { color: var(--pd-danger-text, var(--pd-danger)); }
  `,
})
export class GalData {
  protected readonly plugins = PLUGINS;
  protected readonly none: Plugin[] = [];
  protected readonly many = MANY;
  protected readonly columns = COLUMNS;
  protected readonly key = (p: Plugin): string => p.id;
  protected readonly sort = signal<PdSort | null>(null);
  protected readonly selected = signal<string | null>(null);
  protected readonly opened = signal('');
  protected readonly chosen = signal('');
  protected readonly actions = (p: Plugin): readonly PdRowAction[] => [
    { id: 'reveal', label: 'Reveal in folder', icon: 'folder', inline: true },
    { id: 'copy', label: 'Copy path', icon: 'copy', inline: true },
    { id: p.state === 'disabled' ? 'enable' : 'disable', label: p.state === 'disabled' ? 'Enable' : 'Disable' },
    { id: 'remove', label: 'Remove…', icon: 'trash', danger: true },
  ];
  protected describe(e: PdRowActionEvent<Plugin>): string {
    return `${e.id} ${e.row.name}`;
  }

  protected readonly hours = Array.from({ length: 12 }, (_, i) => `${String(i * 2).padStart(2, '0')}:00`);
  protected readonly line: PdChartSpec = { kind: 'line', x: this.hours, unit: 'req/h', series: [{ name: 'Requests', values: [12, 18, 9, 22, 41, 38, 52, null, 47, null, 21, 15], color: 'accent' }] };
  protected readonly stacked: PdChartSpec = {
    kind: 'stackedBar',
    x: ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'],
    unit: 'jobs',
    series: [
      { name: 'Succeeded', values: [14, 18, 11, 20, 17, 6, 4], color: 'neutral' },
      { name: 'Failed', values: [1, 0, 3, 1, 2, 0, 1], color: 'danger' },
    ],
  };
  protected readonly area: PdChartSpec = { kind: 'area', x: this.hours, unit: 'ms', series: [{ name: 'p95 latency', values: [220, 240, 210, 260, 480, 420, 390, 360, 340, 300, 270, 250] }] };
  protected readonly donut: PdChartSpec = { kind: 'donut', x: ['Logs', 'Models', 'Cache', 'Other'], unit: 'GB', series: [{ name: 'Disk', values: [42, 27, 14, 5] }] };

  protected readonly burstCentre = signal<string | null>(null);
  protected readonly bytes = (v: number): string => `${(v / 10).toFixed(1)} GB`;
  protected readonly tree: PdSunburstNode = {
    id: 'vol',
    label: 'Macintosh HD',
    value: 1_000,
    detail: '1,116,387 items',
    children: [
      { id: 'users', label: 'Users', value: 410, detail: '812,004 items', children: [
        { id: 'alex', label: 'alex', value: 330, children: [
          { id: 'lib', label: 'Library', value: 180, children: [{ id: 'dev', label: 'Developer', value: 95 }, { id: 'caches', label: 'Caches', value: 60 }, { id: 'mail', label: 'Mail', value: 25 }] },
          { id: 'docs', label: 'Documents', value: 90, children: [{ id: 'proj', label: 'Projects', value: 60 }, { id: 'misc', label: 'Misc', value: 30 }] },
          { id: 'dl', label: 'Downloads', value: 60 },
        ] },
        { id: 'shared', label: 'Shared', value: 80 },
      ] },
      { id: 'apps', label: 'Applications', value: 190, children: [{ id: 'xcode', label: 'Xcode', value: 120 }, { id: 'pro', label: 'Pro apps', value: 45 }, { id: 'small', label: 'Small apps', value: 25 }] },
      { id: 'sys', label: 'System', value: 120, children: [{ id: 'syslib', label: 'Library', value: 80 }, { id: 'frame', label: 'Frameworks', value: 40 }] },
      { id: 'opt', label: 'opt', value: 70 },
      { id: 'priv', label: 'private', value: 40 },
      { id: 'tiny1', label: 'tmp', value: 3 },
      { id: 'tiny2', label: 'cores', value: 2 },
      { id: 'other', label: 'Other folders', value: 15, kind: 'other' },
      { id: 'hidden', label: 'Hidden space', value: 80, kind: 'hidden', detail: 'Snapshots and files this app cannot read' },
      { id: 'pending', label: 'Not counted yet', value: 70, kind: 'pending' },
    ],
  };

  protected readonly model = signal({ name: '', port: 8080 });
  protected readonly f = form(this.model, (p) => {
    required(p.name);
    minLength(p.name, 3);
    maxLength(p.name, 24);
    min(p.port, 1);
    validate(p.port, ({ value }) => (value() > 65535 ? { kind: 'max', max: 65535 } : undefined));
  });
  protected readonly submitted = signal(false);
  protected readonly saved = signal('');
  protected submit(e: Event): void {
    e.preventDefault();
    this.submitted.set(true);
    if (this.f().valid()) this.saved.set(this.model().name);
  }
}
