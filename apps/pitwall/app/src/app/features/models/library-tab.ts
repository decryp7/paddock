import { CdkConnectedOverlay, CdkOverlayOrigin } from '@angular/cdk/overlay';
import { ChangeDetectionStrategy, Component, computed, inject, input, output, signal } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { LocalModel } from '../../core/api.types';
import { ago, bytes, bytesIn, bytesUnit, date, seconds, slotsText, varies } from '../../shared/format';
import { CAPABILITY_ICON, CAPABILITY_LABEL, UPDATE_KIND_LABEL, fitLook } from '../../shared/labels';
import { confirm } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { DetailPanel } from '../../shared/ui/detail-panel';
import { Fact } from '../../shared/ui/fact';
import { Panel } from '../../shared/ui/panel';
import { ItemsMenu, MoreButton, RowMenu, type MenuItem } from '../../shared/ui/menu';
import { fromControl, moveRowFocus } from '../../shared/ui/row-nav';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
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
  imports: [MatButton, MatIconButton, MatIconModule, MatTableModule, MatTooltipModule, CdkConnectedOverlay, CdkOverlayOrigin, Panel, SuEmptyState, SlotsPanel, DetailPanel, Fact, ItemsMenu, MoreButton, RowMenu, PdSelect],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Disk" icon="disk" [loading]="store.loading()" [error]="store.disk() ? null : store.error()" [ready]="!!store.disk()" [skeletonLines]="2" (retry)="store.load()">
      <div panel-actions>
        <button type="button" matButton [disabled]="store.unusedModels().length === 0 || store.isBusy()('cleanup')" (click)="cleanUp()" [matTooltip]="cleanupHint()">
          <mat-icon svgIcon="pw:broom" aria-hidden="true" />Clean up unused
        </button>
      </div>
      @if (store.disk(); as d) {
        <div class="disk" role="img" [attr.aria-label]="diskLabel()">
          @for (s of segments(); track s.key) {
            <span class="part" [attr.data-k]="s.key" [style.flex-grow]="s.bytes"></span>
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
          <pd-empty-state heading="No models yet" description="Search Hugging Face for a GGUF model; Pitwall tells you which quantisation fits this machine.">
            <mat-icon slot="icon" svgIcon="pw:models" aria-hidden="true" />
            <button slot="action" type="button" matButton="filled" (click)="discover.emit()"><mat-icon svgIcon="pw:search" aria-hidden="true" />Discover models</button>
          </pd-empty-state>
        } @else {
          <div class="toolbar">{{ summary() }}</div>
          <pw-items-menu #rowMenu label="Model actions" />
          <div class="scroll">
            <table mat-table [dataSource]="store.library()" [trackBy]="byId" [fixedLayout]="true" aria-label="Models in the library">
              <ng-container matColumnDef="model">
                <th mat-header-cell *matHeaderCellDef scope="col">Model</th>
                <td mat-cell *matCellDef="let row" class="model">
                  @let m = asModel(row);
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
              </ng-container>
              <ng-container matColumnDef="size">
                <th mat-header-cell *matHeaderCellDef scope="col" class="right">Size {{ sizeUnit() }}</th>
                <td mat-cell *matCellDef="let m" class="right num">{{ sizeIn(m.sizeBytes, sizeUnit()) }}</td>
              </ng-container>
              <ng-container matColumnDef="params">
                <th mat-header-cell *matHeaderCellDef scope="col" class="right">Params</th>
                <td mat-cell *matCellDef="let m" class="right num">{{ params(m.paramsB) }}</td>
              </ng-container>
              <ng-container matColumnDef="context">
                <th mat-header-cell *matHeaderCellDef scope="col" class="right">Context tokens</th>
                <td mat-cell *matCellDef="let m" class="right num">{{ ctx(m.contextMax) }}</td>
              </ng-container>
              <ng-container matColumnDef="status">
                <th mat-header-cell *matHeaderCellDef scope="col">Status</th>
                <td mat-cell *matCellDef="let m">
                  @if (m.loaded) {
                    <span class="status" data-tone="ok">Loaded</span>
                  } @else {
                    <span class="status" [attr.data-tone]="fit(m).tone" [matTooltip]="m.fit.message">{{ fit(m).label }}</span>
                  }
                  @if (m.pinned) {
                    <mat-icon class="pinned" svgIcon="pw:pin" role="img" aria-label="Pinned" matTooltip="Pinned: never released automatically" />
                  }
                </td>
              </ng-container>
              <ng-container matColumnDef="act">
                <th mat-header-cell *matHeaderCellDef scope="col" class="act"><span class="sr-only">Actions</span></th>
                <td mat-cell *matCellDef="let m" class="act">
                  @if (m.loaded) {
                    <button type="button" matIconButton class="reveal" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.unloadModel(m.id)" [attr.aria-label]="'Unload ' + m.name" matTooltip="Unload"><mat-icon svgIcon="pw:release" aria-hidden="true" /></button>
                  } @else {
                    <button type="button" matIconButton class="reveal" [disabled]="store.isBusy()('model:' + m.id) || m.fit.verdict === 'wontFit'" (click)="store.loadModel(m.id)" [attr.aria-label]="'Load ' + m.name" matTooltip="Load"><mat-icon svgIcon="pw:play" aria-hidden="true" /></button>
                  }
                  <pw-more-button [menu]="rowMenu.menu()" [items]="menuItems(m)" [label]="'More actions for ' + m.name" />
                </td>
              </ng-container>
              <tr mat-header-row *matHeaderRowDef="columns(); sticky: true"></tr>
              <tr
                mat-row
                *matRowDef="let m; columns: columns()"
                tabindex="0"
                [id]="'row-' + m.id"
                [class.selected]="selectedId() === m.id"
                [attr.aria-selected]="selectedId() === m.id"
                (click)="onRowClick(m, $event)"
                (keydown)="onRowKey(m, $event)"
                pwRowMenu
                [matContextMenuTriggerFor]="rowMenu.menu()"
                [matContextMenuTriggerData]="{ items: menuItems(m) }"
              ></tr>
            </table>
          </div>
        }
      </pw-panel>

      @if (selected(); as m) {
        <pw-detail-panel [heading]="m.name" [sub]="m.repo" (closed)="close()">
          <pw-items-menu #panelMenu label="Model actions" />
          <pw-more-button panel-actions [menu]="panelMenu.menu()" [items]="menuItems(m)" [label]="'More actions for ' + m.name" />

          <div panel-toolbar>
            @if (m.loaded) {
              <button type="button" matButton="outlined" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.unloadModel(m.id)">Unload</button>
            } @else {
              <button type="button" matButton="outlined" [disabled]="store.isBusy()('model:' + m.id) || m.fit.verdict === 'wontFit'" (click)="store.loadModel(m.id)">Load</button>
            }
            <button type="button" matButton [disabled]="store.isBusy()('model:' + m.id)" (click)="store.pinModel(m.id, !m.pinned)">
              <mat-icon [svgIcon]="m.pinned ? 'pw:unpin' : 'pw:pin'" aria-hidden="true" />{{ m.pinned ? 'Unpin' : 'Pin (keep loaded)' }}
            </button>
          </div>

          @for (u of updates.noticesFor()(m.id); track u.id) {
            <div class="callout" role="note">
              <p><b>Update available: {{ updateKind[u.kind] }}</b></p>
              <p>{{ u.summary }}</p>
              <button type="button" matButton="outlined" [disabled]="updates.isBusy()(u.id)" [attr.aria-label]="'Get ' + u.file + ' for ' + m.name" (click)="updates.get(u)">Get it</button>
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
              <pd-select [inputId]="'idle-' + m.id" [options]="idleChoices(m)" [value]="m.idleSecs === null ? '' : '' + m.idleSecs" [disabled]="m.pinned || store.isBusy()('model:' + m.id)" [ariaLabel]="'Release ' + m.name + ' when idle'" (valueChange)="setIdle(m, $event)" />
              @if (m.pinned) {
                <span class="hint">Pinned models are never released automatically.</span>
              }
            </div>

            @if (store.enginePins() && store.pinnableBuilds().length) {
              <div class="field">
                <span>Engine build</span>
                <pd-select [inputId]="'pin-' + m.id" [options]="pinChoices()" [value]="store.pinOf()(m.id)?.buildId ?? ''" [disabled]="store.isBusy()('pin:' + m.id)" [ariaLabel]="'Engine build for ' + m.name" (valueChange)="setPin(m, $event)" />
                @if (store.pinOf()(m.id); as pin) {
                  <span class="hint pinned-build" [matTooltip]="m.loaded ? 'Loaded models keep their engine until they are loaded again' : 'Used from the next load'">Pinned to {{ pin.buildId }}{{ pin.tag ? ' ' + pin.tag : '' }}</span>
                }
              </div>
            }

            <div class="field">
              <span>Parallel slots</span>
              <button
                type="button"
                matButton="outlined"
                class="slots-btn"
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
      gap: var(--pd-space-3);
    }
    .disk {
      display: flex;
      gap: 1px;
      height: 8px;
      border-radius: var(--pd-radius-pill);
      overflow: hidden;
      .part {
        flex-basis: 0;
        min-width: 3px;
      }
    }
    [data-k='models'] {
      --seg: var(--pd-chart-1);
    }
    [data-k='engines'] {
      --seg: var(--pd-chart-2);
    }
    [data-k='partial'] {
      --seg: var(--pd-chart-3);
    }
    [data-k='free'] {
      --seg: var(--pd-surface-2);
    }
    .disk .part {
      background: var(--seg);
    }
    /* Direct labels under the bar, in the bar's order; each takes at least its own width. */
    .disk-labels {
      display: flex;
      gap: var(--pd-space-4);
      margin: var(--pd-space-2) 0 0;
      padding: 0;
      list-style: none;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
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
        box-shadow: inset 0 0 0 1px var(--pd-border-strong);
      }
      li:not([data-k='free']) i {
        box-shadow: none;
      }
      b {
        font-weight: var(--pd-weight-medium);
        color: var(--pd-text);
      }
    }
    .unused {
      margin: var(--pd-space-2) 0 0;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .toolbar {
      display: flex;
      align-items: center;
      height: 44px;
      padding: 0 var(--pd-space-4);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      border-bottom: 1px solid var(--pd-hairline);
    }
    /* The list's width (the detail panel open, a small window) decides which columns it keeps. */
    .scroll {
      container-type: inline-size;
    }
    .mat-column-size {
      width: 80px;
    }
    .mat-column-params {
      width: 72px;
    }
    .mat-column-context {
      width: 112px;
    }
    .mat-column-status {
      width: 132px;
    }
    /* Load or unload and the "..." button, with the cell's padding. */
    .mat-column-act {
      width: 92px;
    }
    /* A narrow list drops the columns the detail panel repeats. */
    @container (max-width: 640px) {
      .mat-column-params,
      .mat-column-context {
        display: none;
      }
    }
    @container (max-width: 500px) {
      .mat-column-size {
        display: none;
      }
    }
    table {
      --mat-table-row-item-container-height: 48px;
    }
    .line {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      min-width: 0;
    }
    .name {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--pd-text-body);
      font-weight: var(--pd-weight-semibold);
    }
    .glyphs {
      display: inline-flex;
      align-items: center;
      gap: 6px;
      flex: none;
      mat-icon {
        width: 14px;
        height: 14px;
        color: var(--pd-text-subtle);
      }
      .upd {
        color: var(--pd-accent);
      }
    }
    .repo {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .pinned {
      width: 13px;
      height: 13px;
      margin-left: 6px;
      vertical-align: -2px;
      color: var(--pd-text-subtle);
    }
    .callout {
      display: grid;
      gap: var(--pd-space-1);
      justify-items: start;
      margin-bottom: var(--pd-space-3);
      padding: var(--pd-space-2) var(--pd-space-3);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      background: var(--pd-surface-2);
      border-left: 3px solid var(--pd-info);
      border-radius: var(--pd-radius-tag);
      b {
        color: var(--pd-text);
      }
    }
    .facts {
      display: grid;
    }
    .settings {
      display: grid;
      gap: var(--pd-space-3);
      margin-top: var(--pd-space-4);
      padding-top: var(--pd-space-3);
      border-top: 1px solid var(--pd-hairline);
    }
    .field {
      display: grid;
      gap: 4px;
      justify-items: start;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .hint {
      color: var(--pd-text-muted);
    }
    .plain {
      font-size: var(--pd-text-body);
      color: var(--pd-text);
    }
    pd-select {
      display: block;
      width: 100%;
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
  private readonly dialogs = inject(DialogService);
  protected readonly size = bytes;
  protected readonly when = ago;
  protected readonly secs = seconds;
  protected readonly params = paramsText;
  protected readonly ctx = contextText;
  protected readonly idleOptions = idleOptions;
  protected readonly capability = CAPABILITY_LABEL;
  protected readonly glyph = CAPABILITY_ICON;
  protected idleChoices(m: LocalModel): PdSelectOption[] {
    return [{ value: '', label: 'Global setting' }, ...idleOptions(m.idleSecs).map((s) => ({ value: '' + s, label: `After ${seconds(s)}` }))];
  }
  protected pinChoices(): PdSelectOption[] {
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
    return [`${lib.length} models · ${loaded} loaded`, ...this.shared()].join(' · ');
  });

  /** What every model shares, said once in the toolbar instead of in a constant column (UI review 2 rule 7): `all 8.2B · all 32k context`. */
  protected readonly shared = computed(() => {
    const lib = this.store.library();
    const cols = this.columns();
    const first = lib[0];
    if (!first || lib.length < 3) return [];
    return [
      cols.includes('params') || first.paramsB === null ? '' : `all ${paramsText(first.paramsB)}`,
      cols.includes('context') || first.contextMax === null ? '' : `all ${contextText(first.contextMax)} context`,
    ].filter(Boolean);
  });

  /** The columns that say something: parameters and context only when they differ between models (UI review 2 rule 7). */
  protected readonly columns = computed(() => {
    const lib = this.store.library();
    return ['model', 'size', ...(varies(lib, (m) => m.paramsB) ? ['params'] : []), ...(varies(lib, (m) => m.contextMax) ? ['context'] : []), 'status', 'act'];
  });
  /** The Size column's unit, in its header. */
  protected readonly sizeUnit = computed(() => bytesUnit(this.store.library().map((m) => m.sizeBytes)));
  protected readonly sizeIn = bytesIn;
  protected readonly byId = (_: number, m: LocalModel) => m.id;
  /** Material's cell templates are untyped: the row as the model it is. */
  protected readonly asModel = (row: LocalModel) => row;

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

  /** The one list behind the row's "..." button, its right-click menu and Shift+F10 (`pwRowMenu`). Delete is only here. */
  protected menuItems(m: LocalModel): MenuItem[] {
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
    } else {
      moveRowFocus(e);
    }
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

  protected async remove(m: LocalModel): Promise<void> {
    const ok = await confirm(this.dialogs, { heading: `Delete ${m.name}?`, message: `${m.file} (${bytes(m.sizeBytes)}) is removed from disk. You can download it again later.`, confirm: { label: 'Delete model', destructive: true } });
    if (ok) {
      if (this.selectedId() === m.id) this.selectedId.set(null);
      await this.store.deleteModel(m.id);
    }
  }

  protected async cleanUp(): Promise<void> {
    const list = this.store.unusedModels();
    const ok = await confirm(this.dialogs, {
      heading: `Delete ${list.length} unused ${list.length === 1 ? 'model' : 'models'}?`,
      message: `${list.map((m) => m.name).join(', ')} — ${bytes(this.store.unusedBytes())} in total. You can download them again later.`,
      confirm: { label: list.length === 1 ? 'Delete model' : 'Delete models', destructive: true },
    });
    if (ok) await this.store.cleanUnused();
  }
}
