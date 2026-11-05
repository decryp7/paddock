import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { InventoryFormat, SoftwareItem } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { fileName, formatAgo, plural } from '../../shared/format';
import { injectNow } from '../../shared/now';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { type Numbered, filterSoftware, hue, initial, numberKey, numbered } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatButton } from '@angular/material/button';
import { SearchField } from '../../shared/search-field';
import { MatMenuModule } from '@angular/material/menu';

/** The installed software, searchable, and a report of the hardware and software saved through the OS save dialog. */
@Component({
  selector: 'dy-extras-inventory',
  imports: [AsyncState, Icon, PdTable, PdCell, MatMenuModule, SearchField, MatButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="bar" role="group" aria-label="Software">
      <dy-search-field label="Search installed software" placeholder="Search installed software" [(value)]="search" />
      <span class="faint muted">{{ count() }}</span>
      <span class="grow"></span>
      <button type="button" matButton="outlined" aria-haspopup="menu" [disabled]="store.exporting()" [matMenuTriggerFor]="exportMenu"><dy-icon name="file" /> {{ store.exporting() ? 'Saving…' : 'Export report…' }}</button>
    </div>

    @if (store.saveError(); as e) {
      <p class="note bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span></p>
    }
    @if (store.saved(); as s) {
      <p class="note" role="status"><dy-icon name="check" /><span>Saved {{ name(s.path) }} to</span><code class="path" tabindex="0">{{ s.path }}</code></p>
    }

    <dy-async [loaded]="!!data()" [error]="store.software().error" what="installed software" [rows]="6" (retry)="store.loadSoftware()">
      @if (data(); as inv) {
        @if (inv.unavailable; as why) {
          <div class="none"><dy-icon name="info" /><strong>The software list is not available</strong><span>{{ why }}</span></div>
        } @else if (inv.items.length === 0) {
          <div class="none"><dy-icon name="layers" /><strong>No software found</strong><span>The system lists no installed programs that Dyno can read.</span></div>
        } @else if (rows().length === 0) {
          <div class="none"><dy-icon name="search" /><strong>Nothing matches “{{ search() }}”</strong><span>Try a shorter name or the publisher.</span></div>
        } @else {
          @if (inv.savedAt !== null) {
            <p class="note" role="status" data-testid="saved-list">
              <dy-icon name="info" />
              <span><strong>Last known list</strong>, read {{ ago(inv.savedAt) }}. Dyno is reading the system again.</span>
              <button type="button" matButton="outlined" (click)="store.loadSoftware()">Refresh</button>
            </p>
          }
          <pd-table label="Installed software" [rows]="numberedRows()" [columns]="columns" [rowKey]="rowNo" [filterable]="false" [maxHeight]="620">
            <ng-template pdCell="name" let-i><span class="app"><span class="tile" aria-hidden="true" [style.background]="tile(i.name)">{{ initial(i.name) }}</span><span class="nm">{{ i.name }}</span></span></ng-template>
          </pd-table>
          @if (inv.truncated) { <p class="note" role="status"><dy-icon name="info" /><span>This system has more packages than Dyno lists; the first {{ inv.items.length }} are shown.</span></p> }
        }
      }
    </dy-async>

    <mat-menu #exportMenu="matMenu" aria-label="Export report">
      <ng-template matMenuContent>
        <button mat-menu-item (click)="export('markdown')"><dy-icon name="file" /> Markdown (readable)</button>
        <button mat-menu-item (click)="export('json')"><dy-icon name="file" /> JSON (for other tools)</button>
      </ng-template>
    </mat-menu>
  `,
  styles: [
    EXTRAS_STYLES,
    `
      .app { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
      .app .tile { flex: none; margin: 0; }
      .nm { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    `,
  ],
})
export class ExtrasInventory implements OnInit {
  protected readonly store = inject(ExtrasStore);
  protected readonly search = signal('');
  protected readonly data = computed(() => this.store.software().data);
  protected readonly rows = computed(() => filterSoftware(this.data()?.items ?? [], this.search()));
  protected readonly count = computed(() => {
    const d = this.data();
    if (!d || d.unavailable) return '';
    return this.rows().length === d.items.length ? plural(d.items.length, 'program') : `${this.rows().length} of ${plural(d.items.length, 'program')}`;
  });
  private readonly now = injectNow();
  protected readonly ago = (at: number): string => formatAgo(at, this.now());
  protected readonly initial = initial;
  protected readonly numberedRows = computed(() => numbered(this.rows()));
  protected readonly rowNo = numberKey;
  protected readonly columns: PdColumn<Numbered<SoftwareItem>>[] = [
    { key: 'name', header: 'Name', value: (i) => i.name, required: true, width: 'minmax(0, 2fr)' },
    { key: 'version', header: 'Version', value: (i) => i.version, text: (i) => i.version ?? '—', width: '200px', hideBelow: 520 },
    { key: 'publisher', header: 'Publisher', value: (i) => i.publisher, text: (i) => i.publisher ?? '—', width: 'minmax(0, 1.5fr)', hideBelow: 640 },
  ];
  protected readonly name = fileName;
  protected readonly tile = (name: string) => `hsl(${hue(name)} 42% 38%)`;

  ngOnInit(): void {
    void this.store.ensure('inventory');
  }

  protected export(format: InventoryFormat): void {
    void this.store.exportReport(format);
  }
}
