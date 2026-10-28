import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import type { InventoryFormat } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ExtrasStore } from '../../state/extras.store';
import { fileName } from '../../state/diagnostics.store';
import { filterSoftware, hue, initial } from './extras-model';
import { EXTRAS_STYLES } from './extras-styles';
import { MatButton } from '@angular/material/button';
import { MatTooltip } from '@angular/material/tooltip';
import { FullTextTip } from '../../shared/full-text-tip';
import { SearchField } from '../../shared/search-field';
import { MatMenuModule } from '@angular/material/menu';

/** The installed software, searchable, and a report of the hardware and software saved through the OS save dialog. */
@Component({
  selector: 'dy-extras-inventory',
  imports: [MatMenuModule, SearchField, FullTextTip, MatTooltip, MatButton, AsyncState, Icon],
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
          <div class="tablewrap">
            <table class="table tbl" aria-label="Installed software">
              <thead><tr><th>Name</th><th class="w-ver">Version</th><th>Publisher</th></tr></thead>
              <tbody>
                @for (i of rows(); track i.name) {
                  <tr>
                    <td><span class="tile" aria-hidden="true" [style.background]="tile(i.name)">{{ initial(i.name) }}</span><span class="nm" [dyFullText]="i.name">{{ i.name }}</span></td>
                    <td class="num">{{ i.version ?? '—' }}</td>
                    <td class="muted"><span class="ell" [dyFullText]="i.publisher ?? ''">{{ i.publisher ?? '—' }}</span></td>
                  </tr>
                }
              </tbody>
            </table>
          </div>
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
  styles: [EXTRAS_STYLES, `.w-ver { width: 200px; } .nm { vertical-align: middle; }`],
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
  protected readonly initial = initial;
  protected readonly name = fileName;
  protected readonly tile = (name: string) => `hsl(${hue(name)} 42% 38%)`;

  ngOnInit(): void {
    void this.store.ensure('inventory');
  }

  protected export(format: InventoryFormat): void {
    void this.store.exportReport(format);
  }
}
