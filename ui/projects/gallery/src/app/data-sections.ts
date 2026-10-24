import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { form, FormField, maxLength, min, minLength, required, validate } from '@angular/forms/signals';
import { PdChart, type PdChartSpec } from '@decryp7/paddock-ui/chart';
import { PdField, PdFieldControl } from '@decryp7/paddock-ui/forms';
import { PdCell, PdTable, type PdColumn, type PdRowAction, type PdRowActionEvent, type PdSort } from '@decryp7/paddock-ui/table';

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
  size: (i * 37) % 4096,
  path: `/data/records/${i}/entry.bin`,
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
  imports: [PdTable, PdCell, PdChart, PdField, PdFieldControl, FormField],
  template: `
    <section class="gal-section" data-testid="table" aria-labelledby="h-table">
      <h2 id="h-table">Data table</h2>
      <p>Sort by a header, filter, choose columns; arrows, Home, End and Enter on a row; Shift+F10 or a right-click opens the row menu. Columns drop out as the table narrows.</p>
      <div class="gal-box" data-testid="table-small">
        <pd-table
          label="Plugins"
          [rows]="plugins"
          [columns]="columns"
          [rowKey]="key"
          [rowActions]="actions"
          [rowHeight]="40"
          [(sort)]="sort"
          [(selectedKey)]="selected"
          (rowActivate)="opened.set($event.name)"
          (action)="chosen.set(describe($event))"
        >
          <ng-template pdCell="state" let-row><span class="gal-state" [attr.data-state]="row.state">{{ row.state }}</span></ng-template>
        </pd-table>
      </div>
      <p data-testid="table-status">Opened: {{ opened() || 'none' }}. Chose: {{ chosen() || 'nothing' }}.</p>
      <h3>Grouped, and 5,000 rows in a virtual viewport</h3>
      <div class="gal-box" data-testid="table-virtual">
        <pd-table label="Records" [rows]="many" [columns]="columns" [rowKey]="key" [rowHeight]="32" groupBy="kind" [maxHeight]="260" [virtual]="true" />
      </div>
    </section>

    <section class="gal-section" data-testid="chart" aria-labelledby="h-pdchart">
      <h2 id="h-pdchart">Chart</h2>
      <p>The wrapper draws a spec in the family theme; each chart has a name and a text twin for screen readers.</p>
      <div class="gal-charts">
        <div class="gal-box"><pd-chart label="Requests per hour" [spec]="line" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Jobs by outcome per day" [spec]="stacked" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Latency by hour" [spec]="area" [height]="200" /></div>
        <div class="gal-box"><pd-chart label="Disk use by kind" [spec]="donut" [height]="200" /></div>
      </div>
    </section>

    <section class="gal-section" data-testid="forms" aria-labelledby="h-forms">
      <h2 id="h-forms">Forms</h2>
      <p>Signal Forms with <code>pd-field</code>: the error appears after the field was left, or on submit.</p>
      <form class="gal-form gal-box" (submit)="submit($event)" novalidate>
        <pd-field label="Name" hint="Shown in the list" [state]="f.name()" [submitted]="submitted()">
          <input class="pd-input" type="text" pdFieldControl [formField]="f.name" autocomplete="off" />
        </pd-field>
        <pd-field label="Port" hint="1 to 65535" [state]="f.port()" [submitted]="submitted()">
          <input class="pd-input" type="number" pdFieldControl [formField]="f.port" />
        </pd-field>
        <div class="gal-row"><button class="pd-btn pd-btn--primary" type="submit">Save</button></div>
      </form>
      <p data-testid="forms-status">{{ saved() ? 'Saved ' + saved() : 'Not saved' }}</p>
    </section>
  `,
  styles: `
    .gal-charts { display: grid; grid-template-columns: repeat(auto-fit, minmax(min(100%, 20rem), 1fr)); gap: var(--pd-space-3); }
    .gal-form { display: grid; gap: var(--pd-space-3); padding: var(--pd-space-4); max-width: 28rem; }
    .gal-state { color: var(--pd-text-muted); }
    .gal-state[data-state='failed'] { color: var(--pd-danger-text, var(--pd-danger)); }
  `,
})
export class GalData {
  protected readonly plugins = PLUGINS;
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
  protected readonly line: PdChartSpec = { kind: 'line', x: this.hours, unit: 'req', series: [{ name: 'Requests', values: [12, 18, 9, 22, 41, 38, 52, null, 47, 33, 21, 15], color: 'accent' }] };
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
