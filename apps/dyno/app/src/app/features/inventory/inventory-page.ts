import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { PdCell, PdTable, type PdColumn } from '@decryp7/paddock-ui/table';
import type { Hint, HintKind, MemoryArray } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatCount, formatGhz, formatStorage } from '../../shared/format';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';
import { InventoryStore } from '../../state/inventory.store';
import { ChipPackage } from './chip-package';
import { DeviceArt, type DeviceArtKind } from './device-art';
import { driveKind } from './device-geometry';
import { MemorySlots } from './memory-slots';
import { ReadingSource } from '../../core/reading-source';
import { isAppleSilicon } from './package-geometry';
import { toDimmSlots, toSocPackage } from './hardware-adapter';
import { type SlotView, describeSlots, kindLabel, slotModel, speedLabel } from './slot-model';
import { SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';

const HINT_ICON: Record<HintKind, IconName> = {
  mismatchedSpeed: 'alert',
  belowRated: 'alert',
  mismatchedSize: 'alert',
  singleChannel: 'info',
  multiChannel: 'check',
  freeSlots: 'plus',
  maxCapacity: 'info',
  notUpgradeable: 'lock',
  unknown: 'info',
};
const HINT_TONE: Record<HintKind, string> = {
  mismatchedSpeed: 'warn',
  belowRated: 'warn',
  mismatchedSize: 'warn',
  singleChannel: 'info',
  multiChannel: 'ok',
  freeSlots: 'accent',
  maxCapacity: 'info',
  notUpgradeable: 'muted',
  unknown: 'info',
};

/** What is installed: the memory (slots drawn as sockets, or Apple silicon's unified memory) with upgrade hints from the engine. */
@Component({
  selector: 'dy-inventory-page',
  imports: [SuSegmentedControl, AsyncState, ChipPackage, DeviceArt, Icon, MemorySlots, PdTable, PdCell, ReadingSource],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Inventory</h1>
          <p>What is installed in this computer, and what could be upgraded. Dyno only reads it.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.inventory()" [error]="store.error()" what="inventory" [rows]="4">
        @if (store.inventory(); as inv) {
          <section class="card" aria-label="Memory">
            <div class="card-head">
              <h2><dy-icon name="memory" /> Memory</h2>
              <span class="spacer"></span>
              @if (slots(); as a) {
                <pd-segmented-control label="View" [options]="viewSegs" [value]="table() ? 'table' : 'slots'" (valueChange)="table.set($event === 'table')" />
              }
            </div>

            <dy-reading-source [via]="inv.memoryVia" />

            @if (slots(); as a) {
              <dl class="stats">
                <div><dt>Installed</dt><dd class="num">{{ bytes(model().totalBytes) }}</dd></div>
                <div><dt>Slots in use</dt><dd class="num">{{ n(model().filled) }} of {{ n(model().slots.length) }}</dd></div>
                <div><dt>Free slots</dt><dd class="num">{{ n(model().free) }}</dd></div>
                <div><dt>Board maximum</dt><dd class="num">{{ a.maxCapacityBytes ? bytes(a.maxCapacityBytes) : 'Not reported' }}</dd></div>
              </dl>
              @if (!table()) {
                <div class="drawing"><dy-memory-slots [slots]="dimmSlots()" [alt]="alt()" /></div>
              } @else {
                <div class="tablewrap">
                  <pd-table [toolbar]="false" label="Memory modules" [rows]="model().slots" [columns]="slotColumns" [rowKey]="slotKey" [filterable]="false" height="window">
                    <ng-template pdCell="size" let-s>@if (s.module) {<span class="num">{{ bytes(s.module.capacityBytes) }}</span>} @else {<span class="faint">Empty</span>}</ng-template>
                    <ng-template pdCell="part" let-s><span class="mono">{{ s.module?.partNumber ?? (s.module ? '—' : '') }}</span></ng-template>
                  </pd-table>
                </div>
              }
            } @else if (unified(); as u) {
              <div class="chipwrap">
                <dy-chip-package class="soc" [model]="soc()!" />
              </div>
            } @else if (totalOnly(); as t) {
              <div class="na">
                <strong>{{ bytes(t.total) }} installed</strong>
                <p>Dyno could not read the individual modules: {{ t.reason.reason }}</p>
              </div>
            }

            @if (inv.hints.length) {
              <ul class="hints" aria-label="Upgrade hints">
                @for (h of inv.hints; track $index) {
                  <li [attr.data-tone]="tone(h)">
                    <span class="ic"><dy-icon [name]="icon(h)" /></span>
                    <span>{{ h.message }}</span>
                  </li>
                }
              </ul>
            }
          </section>

          <section class="card" aria-label="This computer">
            <div class="card-head"><h2><dy-icon name="cpu" /> Processor and storage</h2></div>
            <div class="devices">
              <article class="device">
                <dy-device-art class="art" data-art="cpu" kind="cpu" />
                <div class="meta">
                  <span class="k">Processor</span>
                  <strong>{{ inv.cpu.name }}</strong>
                  <span class="sub"><span class="num">{{ n(inv.cpu.physicalCores) }}</span> physical, <span class="num">{{ n(inv.cpu.logicalCores) }}</span> logical cores · <span class="num">{{ ghz(inv.cpu.frequencyMhz) }}</span></span>
                </div>
              </article>
              @for (d of inv.disks; track d.name) {
                <article class="device">
                  <dy-device-art class="art" [attr.data-art]="art(d)" [kind]="art(d)" [totalBytes]="d.totalBytes" />
                  <div class="meta">
                    <span class="k">Drive</span>
                    <strong>{{ d.name }}</strong>
                    <span class="sub"><span class="num">{{ storage(d.totalBytes) }}</span>{{ d.removable ? ' · removable' : '' }}</span>
                  </div>
                </article>
              }
            </div>
          </section>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; --hw-focus: var(--focus); }
    .page { display: grid; gap: var(--sp-4); align-content: start; }
    .page-head { margin-bottom: 0; }
    .card { padding-bottom: var(--sp-4); }
    .stats { margin: 0; padding: var(--sp-3) var(--sp-4) var(--sp-2); display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--sp-3); }
    .stats dt { color: var(--text-3); font-size: var(--fs-sm); }
    .stats dd { margin: 2px 0 0; font: var(--fw-semibold) 28px / 1.2 var(--font-display); }
    .devices { display: grid; grid-template-columns: repeat(auto-fill, minmax(320px, 1fr)); gap: var(--sp-3); padding: var(--sp-3) var(--sp-4) 0; }
    .device { display: flex; align-items: center; gap: var(--sp-4); padding: var(--sp-3) var(--sp-4); border-radius: var(--r-lg); background: var(--surface-inset); min-width: 0; }
    .device .art { flex: none; width: 112px; display: grid; place-items: center; }
    .device .art[data-art='nvme'] { width: 144px; }
    .device .art[data-art='cpu'] { width: 72px; margin-inline: 20px; }
    .device .art[data-art='sata'] { width: 88px; margin-inline: 12px; }
    .device .meta { display: grid; gap: 2px; min-width: 0; }
    .device .k { color: var(--text-3); font-size: var(--fs-xs); line-height: 16px; }
    .device strong { font: var(--fw-semibold) var(--fs-base) / 20px var(--font-sans); overflow-wrap: anywhere; }
    .device .sub { color: var(--text-2); font-size: var(--fs-sm); line-height: 18px; }
    .drawing { padding: var(--sp-3) var(--sp-4); }
    .tablewrap { padding: var(--sp-2) var(--sp-4); }
    .mono { font: 12px/1.4 var(--font-mono); }
    .faint { color: var(--text-3); }
    .chipwrap { display: flex; align-items: center; gap: var(--sp-6); padding: var(--sp-4); flex-wrap: wrap; }
    .chipwrap .soc { flex: 1 1 520px; min-width: 0; }
    .txt { display: grid; gap: var(--sp-1); max-width: 44ch; }
    .txt p, .na p { color: var(--text-2); }
    .na { display: grid; gap: var(--sp-1); padding: var(--sp-4); }
    .hints { list-style: none; margin: var(--sp-2) var(--sp-4) 0; padding: 0; display: grid; border: 1px solid var(--border); border-left: 3px solid var(--text-3); border-radius: var(--r-md); background: var(--surface-inset); }
    .hints:has([data-tone='warn']) { border-left-color: var(--warn); }
    .hints li { display: flex; gap: var(--sp-3); align-items: flex-start; padding: var(--sp-2) var(--sp-3); font-size: var(--fs-sm); }
    .hints li + li { border-top: 1px solid var(--border-subtle); }
    .hints .ic { display: grid; place-items: center; width: 18px; height: 18px; flex: none; color: var(--text-3); }
    .hints [data-tone='warn'] .ic { color: var(--warn); }
    @media (max-width: 760px) { .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  `,
})
export class InventoryPage implements OnInit {
  protected readonly store = inject(InventoryStore);
  protected readonly viewSegs: SuSegment[] = [{ value: 'slots', label: 'Slots' }, { value: 'table', label: 'Table' }];
  protected readonly table = signal(false);
  protected readonly ghz = formatGhz;
  protected readonly isApple = isAppleSilicon;
  protected readonly art = (d: { name: string; removable: boolean }): DeviceArtKind => driveKind(d.name, d.removable);
  protected readonly bytes = formatBytes;
  protected readonly storage = formatStorage;
  protected readonly n = formatCount;
  protected readonly kind = kindLabel;
  protected readonly slotKey = (s: SlotView): string => String(s.index);
  protected readonly slotColumns: PdColumn<SlotView>[] = [
    { key: 'slot', header: 'Slot', value: (s) => s.name, required: true, sortable: false, width: 'minmax(0, 1fr)' },
    { key: 'size', header: 'Size', value: (s) => s.module?.capacityBytes ?? null, text: (s) => (s.module ? formatBytes(s.module.capacityBytes) : 'Empty'), align: 'end', sortable: false, width: '90px' },
    { key: 'type', header: 'Type', value: (s) => (s.module ? kindLabel(s.module.kind) : ''), sortable: false, width: '90px' },
    { key: 'speed', header: 'Speed', value: (s) => (s.module ? speedLabel(s.module) : ''), align: 'end', sortable: false, width: '110px' },
    { key: 'maker', header: 'Maker', value: (s) => (s.module ? (s.module.manufacturer ?? '—') : ''), sortable: false, width: 'minmax(0, 1fr)', hideBelow: 560 },
    { key: 'part', header: 'Part number', value: (s) => (s.module ? (s.module.partNumber ?? '—') : ''), sortable: false, width: 'minmax(0, 1.4fr)', hideBelow: 700 },
  ];
  protected readonly speed = speedLabel;
  protected readonly slots = computed<MemoryArray | null>(() => {
    const m = this.store.inventory()?.memory;
    return m && 'slots' in m ? m.slots : null;
  });
  protected readonly unified = computed(() => {
    const m = this.store.inventory()?.memory;
    return m && 'unified' in m ? m.unified : null;
  });
  protected readonly totalOnly = computed(() => {
    const m = this.store.inventory()?.memory;
    return m && 'totalOnly' in m ? m.totalOnly : null;
  });
  protected readonly model = computed(() => slotModel(this.slots() ?? { slots: 0, maxCapacityBytes: null, modules: [] }, this.store.inventory()?.hints ?? []));
  protected readonly dimmSlots = computed(() => toDimmSlots(this.model()));
  protected readonly alt = computed(() => describeSlots(this.model()));
  protected readonly soc = computed(() => {
    const inv = this.store.inventory();
    const u = this.unified();
    return inv && u ? toSocPackage(inv.cpu, u) : null;
  });
  protected readonly icon = (h: Hint) => HINT_ICON[h.kind];
  protected readonly tone = (h: Hint) => HINT_TONE[h.kind];

  ngOnInit(): void {
    void this.store.load();
  }
}
