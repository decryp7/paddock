import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import type { Hint, HintKind, MemoryArray } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatCount } from '../../shared/format';
import { Icon } from '../../shared/icon';
import type { IconName } from '../../shared/icons';
import { InventoryStore } from '../../state/inventory.store';
import { MemorySlots } from './memory-slots';
import { kindLabel, slotModel, speedLabel } from './slot-model';

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
  imports: [AsyncState, Icon, MemorySlots],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Inventory</h1>
          <p>What is installed in this PC, and what could be upgraded. Dyno only reads it.</p>
        </div>
      </header>

      <dy-async [loaded]="!!store.inventory()" [error]="store.error()" what="inventory" [rows]="4" (retry)="store.load()">
        @if (store.inventory(); as inv) {
          <section class="card" aria-label="Memory">
            <div class="card-head">
              <h2><dy-icon name="memory" /> Memory</h2>
              <span class="spacer"></span>
              @if (slots(); as a) {
                <div class="seg" role="group" aria-label="View">
                  <button type="button" [attr.aria-pressed]="!table()" (click)="table.set(false)">Slots</button>
                  <button type="button" [attr.aria-pressed]="table()" (click)="table.set(true)">Table</button>
                </div>
              }
            </div>

            @if (slots(); as a) {
              <dl class="stats">
                <div><dt>Installed</dt><dd class="num">{{ bytes(model().totalBytes) }}</dd></div>
                <div><dt>Slots in use</dt><dd class="num">{{ n(model().filled) }} of {{ n(model().slots.length) }}</dd></div>
                <div><dt>Free slots</dt><dd class="num">{{ n(model().free) }}</dd></div>
                <div><dt>Board maximum</dt><dd class="num">{{ a.maxCapacityBytes ? bytes(a.maxCapacityBytes) : 'Not reported' }}</dd></div>
              </dl>
              @if (!table()) {
                <div class="drawing"><dy-memory-slots [array]="a" [hints]="inv.hints" /></div>
              } @else {
                <div class="tablewrap">
                  <table class="table">
                    <caption class="visually-hidden">Memory modules</caption>
                    <thead><tr><th>Slot</th><th class="r">Size</th><th>Type</th><th class="r">Speed</th><th>Maker</th><th>Part number</th></tr></thead>
                    <tbody>
                      @for (s of model().slots; track s.index) {
                        <tr>
                          <td>{{ s.name }}</td>
                          @if (s.module; as m) {
                            <td class="r num">{{ bytes(m.capacityBytes) }}</td><td>{{ kind(m.kind) }}</td><td class="r num">{{ speed(m) }}</td><td>{{ m.manufacturer ?? '—' }}</td><td class="mono">{{ m.partNumber ?? '—' }}</td>
                          } @else {
                            <td colspan="5" class="faint">Empty</td>
                          }
                        </tr>
                      }
                    </tbody>
                  </table>
                </div>
              }
            } @else if (unified(); as u) {
              <div class="chipwrap">
                <svg viewBox="0 0 320 170" role="img" [attr.aria-label]="'Unified memory, ' + bytes(u.total) + ' ' + kind(u.kind) + ', part of the chip'">
                  <rect class="die" x="70" y="10" width="180" height="150" rx="22" />
                  <rect class="mem" x="90" y="30" width="60" height="110" rx="10" /><rect class="mem" x="170" y="30" width="60" height="110" rx="10" />
                  <text x="120" y="82" text-anchor="middle" class="big">{{ bytes(u.total) }}</text>
                  <text x="120" y="102" text-anchor="middle" class="small">{{ kind(u.kind) }}</text>
                  <text x="200" y="82" text-anchor="middle" class="big">SoC</text>
                  <text x="200" y="102" text-anchor="middle" class="small">CPU · GPU</text>
                  @for (i of pins; track i) {
                    <rect class="pin" [attr.x]="82 + i * 22" y="2" width="8" height="8" rx="2" /><rect class="pin" [attr.x]="82 + i * 22" y="160" width="8" height="8" rx="2" />
                  }
                </svg>
                <div class="txt">
                  <strong>Unified memory</strong>
                  <p>{{ bytes(u.total) }} {{ kind(u.kind) }}{{ u.manufacturer ? ' by ' + u.manufacturer : '' }}, shared by the CPU and the GPU. It is part of the chip package{{ u.upgradeable ? '.' : ' and cannot be upgraded.' }}</p>
                </div>
              </div>
            } @else if (totalOnly(); as t) {
              <div class="na">
                <strong>{{ bytes(t.total) }} installed</strong>
                <p>Dyno could not read the individual modules: {{ t.reason.reason }}@if (t.reason.needsHelper) { Reading them needs the Dyno helper, which asks for administrator rights the first time.}</p>
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

          <section class="card" aria-label="This PC">
            <div class="card-head"><h2><dy-icon name="cpu" /> Processor and storage</h2></div>
            <dl class="stats two">
              <div><dt>Processor</dt><dd>{{ inv.cpu.name }}</dd></div>
              <div><dt>Cores</dt><dd class="num">{{ n(inv.cpu.physicalCores) }} physical, {{ n(inv.cpu.logicalCores) }} logical</dd></div>
              @for (d of inv.disks; track d.name) {
                <div><dt>Drive</dt><dd>{{ d.name }} · <span class="num">{{ bytes(d.totalBytes) }}</span>{{ d.removable ? ' · removable' : '' }}</dd></div>
              }
            </dl>
          </section>
        }
      </dy-async>
    </div>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 980px; }
    .page-head { margin-bottom: 0; }
    .card { padding-bottom: var(--sp-4); }
    .stats { margin: 0; padding: var(--sp-3) var(--sp-4) var(--sp-2); display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: var(--sp-3); }
    .stats.two { grid-template-columns: repeat(2, minmax(0, 1fr)); }
    .stats dt { color: var(--text-3); font-size: var(--fs-xs); letter-spacing: 0.04em; text-transform: uppercase; }
    .stats dd { margin: 2px 0 0; font: var(--fw-semibold) var(--fs-lg) / 1.3 var(--font-display); }
    .two dd { font-size: var(--fs-base); font-weight: var(--fw-medium); }
    .drawing { padding: var(--sp-3) var(--sp-4); }
    .tablewrap { padding: var(--sp-2) var(--sp-4); overflow-x: auto; }
    .faint { color: var(--text-3); }
    .chipwrap { display: flex; align-items: center; gap: var(--sp-6); padding: var(--sp-4); flex-wrap: wrap; }
    .chipwrap svg { width: 320px; max-width: 100%; font-family: var(--font-sans); }
    .die { fill: var(--surface-3); stroke: var(--border-strong); stroke-width: 1.5; }
    .mem { fill: var(--chart-1); fill-opacity: 0.2; stroke: var(--chart-1); stroke-width: 1.5; }
    .pin { fill: var(--surface-active); }
    .big { font-size: 20px; font-weight: 600; fill: var(--text-1); }
    .small { font-size: 11px; fill: var(--text-2); }
    .txt { display: grid; gap: var(--sp-1); max-width: 44ch; }
    .txt p, .na p { color: var(--text-2); }
    .na { display: grid; gap: var(--sp-1); padding: var(--sp-4); }
    .hints { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-4) 0; display: grid; gap: var(--sp-2); }
    .hints li { display: flex; gap: var(--sp-3); align-items: flex-start; padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); }
    .hints .ic { display: grid; place-items: center; width: 24px; height: 24px; border-radius: var(--r-sm); flex: none; background: var(--surface-active); color: var(--text-2); }
    .hints [data-tone='warn'] .ic { background: var(--warn-soft); color: var(--warn); }
    .hints [data-tone='ok'] .ic { background: var(--ok-soft); color: var(--ok); }
    .hints [data-tone='info'] .ic { background: var(--info-soft); color: var(--info); }
    .hints [data-tone='accent'] .ic { background: var(--accent-soft); color: var(--accent-text); }
    @media (max-width: 760px) { .stats { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  `,
})
export class InventoryPage implements OnInit {
  protected readonly store = inject(InventoryStore);
  protected readonly table = signal(false);
  protected readonly pins = [0, 1, 2, 3, 4, 5, 6];
  protected readonly bytes = formatBytes;
  protected readonly n = formatCount;
  protected readonly kind = kindLabel;
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
  protected readonly icon = (h: Hint) => HINT_ICON[h.kind];
  protected readonly tone = (h: Hint) => HINT_TONE[h.kind];

  ngOnInit(): void {
    void this.store.load();
  }
}
