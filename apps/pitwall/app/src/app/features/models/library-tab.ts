import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { LocalModel } from '../../core/api.types';
import { ago, bytes, date, seconds, slotsText } from '../../shared/format';
import { CAPABILITY_ICON, CAPABILITY_LABEL, UPDATE_KIND_LABEL, fitLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { ContextMenuService, type ContextMenuItem } from '../../shared/ui/context-menu';
import { DetailPanel } from '../../shared/ui/detail-panel';
import { EmptyState } from '../../shared/ui/empty-state';
import { Fact } from '../../shared/ui/fact';
import { Panel } from '../../shared/ui/panel';
import { RowMenu } from '../../shared/ui/row-menu';
import { PwSelect, type SelectOption } from '../../shared/ui/select';
import { fromControl, isMenuKey, moveRowFocus } from '../../shared/ui/row-nav';
import { ModelsStore } from './models.store';
import { UpdatesStore } from '../updates/updates.store';
import { SlotsPanel } from './slots-panel';

/** Per-model idle choices (seconds); `null` follows Settings ▸ Memory. */
export const IDLE_CHOICES: readonly number[] = [60, 120, 300, 600, 1800, 3600];

/** The idle choices for a model, including its current custom value when it is not a preset one. */
export function idleOptions(current: number | null): number[] {
  const list = [...IDLE_CHOICES];
  if (current !== null && !list.includes(current)) list.push(current);
  return list.sort((a, b) => a - b);
}

/** `8.2B`, `140M`. */
export function paramsText(b: number | null): string {
  if (b === null) return '—';
  return b >= 1 ? `${b.toFixed(b >= 10 ? 0 : 1)}B` : `${Math.round(b * 1000)}M`;
}

/** `32k`, `128k`. */
export function contextText(tokens: number | null): string {
  if (tokens === null) return '—';
  return tokens >= 1024 ? `${Math.round(tokens / 1024)}k` : String(tokens);
}

/** The local library: disk usage with clean-up, and every model with load, pin, idle override and delete. */
@Component({
  selector: 'pw-library-tab',
  imports: [MatIconModule, MatTooltipModule, CdkConnectedOverlay, CdkOverlayOrigin, Panel, EmptyState, SlotsPanel, DetailPanel, Fact, RowMenu, PwSelect],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Disk" icon="disk" [loading]="store.loading()" [error]="store.disk() ? null : store.error()" [ready]="!!store.disk()" [skeletonLines]="2" (retry)="store.load()">
      <div panel-actions>
        <button type="button" class="btn ghost sm" [disabled]="store.unusedModels().length === 0 || store.isBusy()('cleanup')" (click)="cleanUp()" [matTooltip]="cleanupHint()">
          <mat-icon svgIcon="pw:broom" aria-hidden="true" />Clean up unused
        </button>
      </div>
      @if (store.disk(); as d) {
        <div class="disk" role="img" [attr.aria-label]="diskLabel()">
          @for (s of segments(); track s.key) {
            <span class="seg" [attr.data-k]="s.key" [style.flex-grow]="s.bytes"></span>
          }
        </div>
        <ul class="disk-labels">
          @for (s of segments(); track s.key) {
            <li [attr.data-k]="s.key" [style.flex-grow]="s.share"><i></i><span class="what">{{ s.label }}</span> <b class="num">{{ size(s.bytes) }}</b></li>
          }
        </ul>
        @if (store.unusedModels().length) {
          <p class="unused">{{ store.unusedModels().length }} unused for {{ d.unusedAfterDays }}+ days · {{ size(store.unusedBytes()) }}</p>
        }
      }
    </pw-panel>

    <div class="with-panel">
      <pw-panel [flush]="true" [loading]="store.loading()" [error]="store.error()" [ready]="store.loaded()" [skeletonLines]="6" (retry)="store.load()">
        @if (store.library().length === 0) {
          <pw-empty-state icon="models" heading="No models yet" text="Search Hugging Face for a GGUF model; Pitwall tells you which quantisation fits this machine.">
            <button type="button" class="btn primary sm" (click)="discover.emit()"><mat-icon svgIcon="pw:search" aria-hidden="true" />Discover models</button>
          </pw-empty-state>
        } @else {
          <div class="toolbar">{{ summary() }}</div>
          <div class="scroll">
            <table class="table">
              <thead>
                <tr>
                  <th scope="col">Model</th>
                  <th scope="col" class="right col-size">Size</th>
                  <th scope="col" class="right col-params">Params</th>
                  <th scope="col" class="right col-ctx">Context</th>
                  <th scope="col">Status</th>
                  <th scope="col" class="act"><span class="sr-only">Actions</span></th>
                </tr>
              </thead>
              <tbody>
                @for (m of store.library(); track m.id) {
                  <tr
                    tabindex="0"
                    [id]="'row-' + m.id"
                    [class.selected]="selectedId() === m.id"
                    [attr.aria-selected]="selectedId() === m.id"
                    (click)="onRowClick(m, $event)"
                    (keydown)="onRowKey(m, $event)"
                    (contextmenu)="onRowMenu(m, $event)"
                  >
                    <td class="model">
                      <span class="line">
                        <span class="name" [matTooltip]="m.name" matTooltipShowDelay="600">{{ m.name }}</span>
                        <span class="glyphs">
                          @for (c of m.capabilities; track c) {
                            <mat-icon [svgIcon]="'pw:' + glyph[c]" role="img" [attr.aria-label]="capability[c]" [matTooltip]="capability[c]" />
                          }
                          @if (updates.noticesFor()(m.id).length) {
                            <mat-icon class="upd" svgIcon="pw:download" role="img" [attr.aria-label]="'Update available for ' + m.name" [matTooltip]="'Update available: ' + updateKind[updates.noticesFor()(m.id)[0]!.kind]" />
                          }
                        </span>
                      </span>
                      <span class="repo" [matTooltip]="m.repo" matTooltipShowDelay="600">{{ m.repo }}</span>
                    </td>
                    <td class="right num col-size">{{ size(m.sizeBytes) }}</td>
                    <td class="right num col-params">{{ params(m.paramsB) }}</td>
                    <td class="right num col-ctx">{{ ctx(m.contextMax) }}</td>
                    <td>
                      @if (m.loaded) {
                        <span class="status" data-tone="ok">Loaded</span>
                      } @else {
                        <span class="status" [attr.data-tone]="fit(m).tone" [matTooltip]="m.fit.message">{{ fit(m).label }}</span>
                      }
                      @if (m.pinned) {
                        <mat-icon class="pinned" svgIcon="pw:pin" role="img" aria-label="Pinned" matTooltip="Pinned: never released automatically" />
                      }
                    </td>
                    <td class="act">
                      <span class="row-actions">
                        @if (m.loaded) {
                          <button type="button" class="btn sm" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.unloadModel(m.id)" [attr.aria-label]="'Unload ' + m.name">Unload</button>
                        } @else {
                          <button type="button" class="btn sm" [disabled]="store.isBusy()('model:' + m.id) || m.fit.verdict === 'wontFit'" (click)="store.loadModel(m.id)" [attr.aria-label]="'Load ' + m.name">Load</button>
                        }
                        <pw-row-menu [items]="menuItems(m)" [label]="'More actions for ' + m.name" />
                      </span>
                    </td>
                  </tr>
                }
              </tbody>
            </table>
          </div>
        }
      </pw-panel>

      @if (selected(); as m) {
        <pw-detail-panel [heading]="m.name" [sub]="m.repo" (closed)="close()">
          <pw-row-menu panel-actions [items]="menuItems(m)" [label]="'More actions for ' + m.name" />

          <div panel-toolbar>
            @if (m.loaded) {
              <button type="button" class="btn" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.unloadModel(m.id)">Unload</button>
            } @else {
              <button type="button" class="btn" [disabled]="store.isBusy()('model:' + m.id) || m.fit.verdict === 'wontFit'" (click)="store.loadModel(m.id)">Load</button>
            }
            <button type="button" class="btn ghost" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.pinModel(m.id, !m.pinned)">
              <mat-icon [svgIcon]="m.pinned ? 'pw:unpin' : 'pw:pin'" aria-hidden="true" />{{ m.pinned ? 'Unpin' : 'Pin (keep loaded)' }}
            </button>
          </div>

          @for (u of updates.noticesFor()(m.id); track u.id) {
            <div class="callout" role="note">
              <p><b>Update available: {{ updateKind[u.kind] }}</b></p>
              <p>{{ u.summary }}</p>
              <button type="button" class="btn sm" [disabled]="updates.isBusy()(u.id)" [attr.aria-label]="'Get ' + u.file + ' for ' + m.name" (click)="updates.get(u)">Get it</button>
            </div>
          }

          <section aria-label="About this model" class="facts">
            <pw-fact label="Status" [value]="statusText(m)"><span class="status" [attr.data-tone]="m.loaded ? 'ok' : fit(m).tone">{{ statusText(m) }}</span></pw-fact>
            <pw-fact label="Repository" [value]="m.repo" [copy]="true" [mono]="true" />
            <pw-fact label="File" [value]="m.file" [copy]="true" [mono]="true" />
            <pw-fact label="Quantisation" [value]="m.quant ?? '—'" />
            <pw-fact label="Size" [value]="size(m.sizeBytes)" [num]="true" />
            <pw-fact label="Parameters" [value]="params(m.paramsB)" [num]="true" />
            <pw-fact label="Context" [value]="ctx(m.contextMax)" [num]="true" />
            @if (m.architecture) {
              <pw-fact label="Architecture" [value]="m.architecture" />
            }
            <pw-fact label="Capabilities" [value]="capabilityText(m)" />
            <pw-fact label="Added" [value]="added(m)" />
            <pw-fact label="Last used" [value]="when(m.lastUsedAt, now())" />
          </section>

          <section aria-label="Settings" class="settings">
            <div class="field">
              <span>Release when idle</span>
<pw-select [inputId]="'idle-' + m.id" [options]="idleChoices(m)" [value]="m.idleSecs === null ? '' : '' + m.idleSecs" [disabled]="m.pinned || store.isBusy()('model:' + m.id)" [label]="'Release ' + m.name + ' when idle'" (changed)="setIdle(m, $event)" />
              @if (m.pinned) {
                <span class="hint">Pinned models are never released automatically.</span>
              }
            </div>

            <div class="field">
              <span>Engine</span>
              @if (store.engineChoices().length) {
                <pw-select [inputId]="'engine-' + m.id" [options]="engineChoicesFor()" [value]="m.engine ?? ''" [disabled]="m.loaded || store.isBusy()('model:' + m.id)" [matTooltip]="m.loaded ? 'Unload the model to change its engine' : ''" [label]="'Engine for ' + m.name" (changed)="setEngine(m, $event)" />
              } @else {
                <span class="plain">llama.cpp</span>
              }
            </div>

            @if (!m.engine && store.enginePins() && store.pinnableBuilds().length) {
              <div class="field">
                <span>Engine build</span>
                <pw-select [inputId]="'pin-' + m.id" [options]="pinChoices()" [value]="store.pinOf()(m.id)?.buildId ?? ''" [disabled]="store.isBusy()('pin:' + m.id)" [label]="'Engine build for ' + m.name" (changed)="setPin(m, $event)" />
                @if (store.pinOf()(m.id); as pin) {
                  <span class="hint pinned-build" [matTooltip]="m.loaded ? 'Loaded models keep their engine until they are loaded again' : 'Used from the next load'">Pinned to {{ pin.buildId }}{{ pin.tag ? ' ' + pin.tag : '' }}</span>
                }
              </div>
            }

            <div class="field">
              <span>Parallel slots</span>
              <button
                type="button"
                class="btn sm slots-btn"
                [id]="'slots-' + m.id"
                cdkOverlayOrigin
                #slotsOrigin="cdkOverlayOrigin"
                [attr.aria-label]="'Parallel slots for ' + m.name + ': ' + slotsLabel(m)"
                aria-haspopup="dialog"
                [attr.aria-expanded]="slotsOpen() === m.id"
                [matTooltip]="slotsHint(m)"
                (click)="toggleSlots(m.id)"
              >
                {{ slotsLabel(m) }}<mat-icon svgIcon="pw:chevron-down" aria-hidden="true" />
              </button>
              <ng-template cdkConnectedOverlay [cdkConnectedOverlayOrigin]="slotsOrigin" [cdkConnectedOverlayOpen]="slotsOpen() === m.id" [cdkConnectedOverlayPush]="true" [cdkConnectedOverlayViewportMargin]="12" (overlayOutsideClick)="slotsOpen.set(null)" (detach)="slotsOpen.set(null)">
                <pw-slots-panel role="dialog" [attr.aria-label]="'Parallel slots for ' + m.name" [model]="m" (closed)="slotsOpen.set(null)" (keydown.escape)="slotsOpen.set(null)" />
              </ng-template>
            </div>
          </section>

        </pw-detail-panel>
      }
    </div>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-3);
    }
    .disk {
      display: flex;
      gap: 1px;
      height: 8px;
      border-radius: var(--r-full);
      overflow: hidden;
      .seg {
        flex-basis: 0;
        min-width: 3px;
      }
    }
    [data-k='models'] {
      --seg: var(--viz-1);
    }
    [data-k='engines'] {
      --seg: var(--viz-2);
    }
    [data-k='partial'] {
      --seg: var(--viz-3);
    }
    [data-k='free'] {
      --seg: var(--track);
    }
    .disk .seg {
      background: var(--seg);
    }
    /* Direct labels under the bar, in the bar's order; each takes at least its own width. */
    .disk-labels {
      display: flex;
      gap: var(--sp-4);
      margin: var(--sp-2) 0 0;
      padding: 0;
      list-style: none;
      font-size: var(--fs-sm);
      color: var(--text-2);
      li {
        flex: 1 1 auto;
        display: inline-flex;
        align-items: center;
        gap: 6px;
        white-space: nowrap;
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
        background: var(--seg);
        box-shadow: inset 0 0 0 1px var(--border-strong);
      }
      li:not([data-k='free']) i {
        box-shadow: none;
      }
      b {
        font-weight: var(--fw-medium);
        color: var(--text);
      }
    }
    .unused {
      margin: var(--sp-2) 0 0;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .toolbar {
      display: flex;
      align-items: center;
      height: 44px;
      padding: 0 var(--sp-4);
      font-size: var(--fs-sm);
      color: var(--text-2);
      border-bottom: 1px solid var(--border-soft);
    }
    .scroll {
      container-type: inline-size;
      overflow-x: auto;
      /* The last row's selected or hover background must not square off the panel's rounded corners. */
      border-radius: 0 0 calc(var(--r-lg) - 1px) calc(var(--r-lg) - 1px);
    }
    /* A narrow list (the detail panel is open, or a small window) drops the columns the panel repeats. */
    @container (max-width: 640px) {
      .col-params,
      .col-ctx {
        display: none;
      }
    }
    @container (max-width: 500px) {
      .col-size {
        display: none;
      }
    }
    /* The action column stays in view whatever the width, so Load / Unload never clip. */
    .table th.act,
    .table td.act {
      position: sticky;
      right: 0;
      z-index: 1;
      width: 1%;
      background: var(--bg-card);
      box-shadow: -10px 0 8px -8px color-mix(in srgb, var(--text) 14%, transparent);
    }
    .table tr:hover td.act {
      background: color-mix(in srgb, var(--bg-card) 94%, var(--text));
    }
    .table tr.selected td.act {
      background: color-mix(in srgb, var(--bg-card) 90%, var(--accent));
    }
    .table th:first-child,
    .table td:first-child {
      padding-left: var(--sp-4);
    }
    .table th:last-child,
    .table td:last-child {
      padding-right: var(--sp-3);
    }
    .table td {
      height: 48px;
    }
    .table td.act {
      min-width: 128px;
    }
    .model {
      width: 100%;
      max-width: 0;
      min-width: 180px;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .line {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      min-width: 0;
    }
    .name {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--fs-base);
      font-weight: var(--fw-semibold);
    }
    .glyphs {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      flex: none;
      mat-icon {
        width: 14px;
        height: 14px;
        color: var(--text-3);
      }
      .upd {
        color: var(--accent);
      }
    }
    .repo {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--fs-xs);
      color: var(--text-2);
    }
    .pinned {
      width: 13px;
      height: 13px;
      margin-left: 6px;
      vertical-align: -2px;
      color: var(--text-3);
    }
    .row-actions .btn.sm:not(.icon) {
      min-width: 64px;
    }
    .callout {
      display: grid;
      gap: var(--sp-1);
      justify-items: start;
      margin-bottom: var(--sp-3);
      padding: var(--sp-2) var(--sp-3);
      font-size: var(--fs-sm);
      color: var(--text-2);
      background: var(--bg-sunken);
      border-left: 3px solid var(--info);
      border-radius: var(--r-sm);
      b {
        color: var(--text);
      }
    }
    .facts {
      display: grid;
    }
    .settings {
      display: grid;
      gap: var(--sp-3);
      margin-top: var(--sp-4);
      padding-top: var(--sp-3);
      border-top: 1px solid var(--border-soft);
    }
    .field {
      display: grid;
      gap: 4px;
      justify-items: start;
      font-size: var(--fs-sm);
      color: var(--text-2);
      .select {
        width: 100%;
        height: 28px;
      }
    }
    .hint {
      color: var(--text-2);
    }
    .plain {
      font-size: var(--fs-base);
      color: var(--text);
    }
    pw-select {
      display: block;
    }
    .slots-btn {
      gap: 2px;
      font-variant-numeric: tabular-nums;
      mat-icon {
        width: 14px;
        height: 14px;
      }
    }
  `,
})
export class LibraryTab {
  readonly now = input.required<number>();
  readonly discover = output<void>();
  protected readonly store = inject(ModelsStore);
  protected readonly updates = inject(UpdatesStore);
  protected readonly updateKind = UPDATE_KIND_LABEL;
  private readonly confirm = inject(Confirm);
  private readonly menu = inject(ContextMenuService);
  protected readonly size = bytes;
  protected readonly when = ago;
  protected readonly secs = seconds;
  protected readonly params = paramsText;
  protected readonly ctx = contextText;
  protected readonly idleOptions = idleOptions;
  protected readonly capability = CAPABILITY_LABEL;
  protected readonly glyph = CAPABILITY_ICON;
  protected idleChoices(m: LocalModel): SelectOption[] {
    return [{ value: '', label: 'Global setting' }, ...idleOptions(m.idleSecs).map((s) => ({ value: '' + s, label: `After ${seconds(s)}` }))];
  }
  protected engineChoicesFor(): SelectOption[] {
    return [{ value: '', label: 'llama.cpp' }, ...this.store.engineChoices().map((e) => ({ value: e.id, label: e.name }))];
  }
  protected pinChoices(): SelectOption[] {
    return [{ value: '', label: 'Follows global choice' }, ...this.store.pinnableBuilds().map((b) => ({ value: b.id, label: `Pin to ${b.label} ${b.tag}` }))];
  }
  protected readonly fit = (m: LocalModel) => fitLook(m.fit.verdict);

  constructor() {
    void this.updates.ensureLoaded();
  }

  protected readonly summary = computed(() => {
    const lib = this.store.library();
    if (!lib.length) return '';
    const loaded = lib.filter((m) => m.loaded).length;
    return `${lib.length} models · ${loaded} loaded`;
  });

  protected readonly segments = computed(() => {
    const d = this.store.disk();
    if (!d) return [];
    const all = [
      { key: 'models', label: 'Models', bytes: d.modelsBytes },
      { key: 'engines', label: 'Engines', bytes: d.enginesBytes },
      { key: 'partial', label: 'Partial downloads', bytes: d.partialBytes },
      { key: 'free', label: 'Free', bytes: d.freeBytes },
    ].filter((s) => s.bytes > 0);
    const total = all.reduce((t, s) => t + s.bytes, 0) || 1;
    return all.map((s) => ({ ...s, share: s.bytes / total }));
  });

  protected readonly diskLabel = computed(() => {
    const d = this.store.disk();
    return d ? `Models ${bytes(d.modelsBytes)}, engines ${bytes(d.enginesBytes)}, partial downloads ${bytes(d.partialBytes)}, ${bytes(d.freeBytes)} free` : '';
  });

  protected readonly cleanupHint = computed(() => {
    const n = this.store.unusedModels().length;
    const days = this.store.disk()?.unusedAfterDays ?? 30;
    return n ? `Delete ${n} ${n === 1 ? 'model' : 'models'} unused for ${days} days (${bytes(this.store.unusedBytes())})` : `No model has been unused for ${days} days`;
  });

  protected readonly slotsOpen = signal<string | null>(null);
  /** The model whose details are open on the right. */
  protected readonly selectedId = signal<string | null>(null);
  protected readonly selected = computed(() => this.store.library().find((m) => m.id === this.selectedId()) ?? null);

  /** `4 slots`, with the default marked: `2 slots (default)`. */
  protected slotsLabel(m: LocalModel): string {
    return m.slotsCustom ? slotsText(m.slots) : `${slotsText(m.slots)} (default)`;
  }

  /** What the button tells on hover, including that a loaded model keeps its count until it loads again. */
  protected slotsHint(m: LocalModel): string {
    if (m.loadedSlots !== null && m.loadedSlots !== m.slots) return `Loaded with ${slotsText(m.loadedSlots)}; ${slotsText(m.slots)} apply the next time it loads`;
    return 'Requests this model serves at once; each slot keeps its own context in memory';
  }

  protected toggleSlots(id: string): void {
    this.slotsOpen.update((open) => (open === id ? null : id));
  }

  protected statusText(m: LocalModel): string {
    return m.loaded ? 'Loaded' : fitLook(m.fit.verdict).label;
  }

  protected capabilityText(m: LocalModel): string {
    return m.capabilities.map((c) => CAPABILITY_LABEL[c]).join(', ') || '—';
  }

  protected added(m: LocalModel): string {
    return date(m.addedAt);
  }

  /** The one list behind the row's "..." button, its right-click menu and Shift+F10. Delete is only here. */
  protected menuItems(m: LocalModel): ContextMenuItem[] {
    const busy = this.store.isBusy()('model:' + m.id);
    return [
      { label: 'Details', run: () => this.selectedId.set(m.id) },
      m.loaded
        ? { label: 'Unload', run: () => void this.store.unloadModel(m.id), disabled: busy }
        : { label: 'Load', run: () => void this.store.loadModel(m.id), disabled: busy || m.fit.verdict === 'wontFit' },
      { label: m.pinned ? 'Unpin' : 'Pin (keep loaded)', run: () => void this.store.pinModel(m.id, !m.pinned), disabled: busy },
      { label: 'Release when idle…', run: () => this.focusSetting(m, 'idle'), disabled: m.pinned },
      { label: 'Parallel slots…', run: () => this.focusSetting(m, 'slots') },
      { label: 'Delete…', run: () => void this.remove(m), danger: true, divider: true, disabled: busy },
    ];
  }

  /** Opens the details and moves focus to one setting, for the menu items that name it. */
  private focusSetting(m: LocalModel, which: 'idle' | 'slots'): void {
    this.selectedId.set(m.id);
    setTimeout(() => document.getElementById(`${which}-${m.id}`)?.focus(), 0);
  }

  protected onRowClick(m: LocalModel, e: Event): void {
    if (!fromControl(e)) this.selectedId.set(m.id);
  }

  protected onRowKey(m: LocalModel, e: KeyboardEvent): void {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      this.selectedId.set(m.id);
    } else if (isMenuKey(e)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      this.menu.open(r.left + 48, r.bottom - 8, this.menuItems(m));
    } else {
      moveRowFocus(e);
    }
  }

  protected onRowMenu(m: LocalModel, e: MouseEvent): void {
    e.preventDefault();
    this.menu.open(e.clientX, e.clientY, this.menuItems(m));
  }

  protected close(): void {
    this.selectedId.set(null);
    this.slotsOpen.set(null);
  }

  protected setIdle(m: LocalModel, value: string): void {
    void this.store.setIdle(m.id, value === '' ? null : Number(value));
  }

  protected setPin(m: LocalModel, value: string): void {
    void this.store.setPin(m.id, value === '' ? null : value);
  }

  protected setEngine(m: LocalModel, value: string): void {
    void this.store.setEngine(m.id, value === '' ? null : value);
  }

  protected async remove(m: LocalModel): Promise<void> {
    const ok = await this.confirm.ask({ title: `Delete ${m.name}?`, message: `${m.file} (${bytes(m.sizeBytes)}) is removed from disk. You can download it again later.`, confirm: 'Delete', danger: true });
    if (ok) {
      if (this.selectedId() === m.id) this.selectedId.set(null);
      await this.store.deleteModel(m.id);
    }
  }

  protected async cleanUp(): Promise<void> {
    const list = this.store.unusedModels();
    const ok = await this.confirm.ask({
      title: `Delete ${list.length} unused ${list.length === 1 ? 'model' : 'models'}?`,
      message: `${list.map((m) => m.name).join(', ')} — ${bytes(this.store.unusedBytes())} in total. You can download them again later.`,
      confirm: 'Delete',
      danger: true,
    });
    if (ok) await this.store.cleanUnused();
  }
}
