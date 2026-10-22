import { ChangeDetectionStrategy, Component, computed, inject, input, output } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { LocalModel } from '../../core/api.types';
import { ago, bytes, seconds } from '../../shared/format';
import { CAPABILITY_LABEL, fitLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { ModelsStore } from './models.store';

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
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <pw-panel heading="Disk" icon="disk" [loading]="store.loading()" [error]="store.disk() ? null : store.error()" [ready]="!!store.disk()" [skeletonLines]="2" (retry)="store.load()">
      <div panel-actions>
        <button type="button" class="btn sm" [disabled]="store.unusedModels().length === 0 || store.isBusy()('cleanup')" (click)="cleanUp()" [matTooltip]="cleanupHint()">
          <mat-icon svgIcon="pw:broom" aria-hidden="true" />Clean up unused
        </button>
      </div>
      @if (store.disk(); as d) {
        <div class="disk" role="img" [attr.aria-label]="diskLabel()">
          @for (s of segments(); track s.key) {
            <span class="seg" [attr.data-k]="s.key" [style.flex-grow]="s.bytes"></span>
          }
        </div>
        <ul class="disk-legend">
          <li><i data-k="models"></i>Models <b class="num">{{ size(d.modelsBytes) }}</b></li>
          <li><i data-k="engines"></i>Engines <b class="num">{{ size(d.enginesBytes) }}</b></li>
          <li><i data-k="partial"></i>Partial downloads <b class="num">{{ size(d.partialBytes) }}</b></li>
          <li><i data-k="free"></i>Free <b class="num">{{ size(d.freeBytes) }}</b></li>
          @if (store.unusedModels().length) {
            <li class="unused">{{ store.unusedModels().length }} unused for {{ d.unusedAfterDays }}+ days · {{ size(store.unusedBytes()) }}</li>
          }
        </ul>
      }
    </pw-panel>

    <pw-panel heading="Library" icon="models" [sub]="summary()" [flush]="true" [loading]="store.loading()" [error]="store.error()" [ready]="store.loaded()" [skeletonLines]="6" (retry)="store.load()">
      @if (store.library().length === 0) {
        <pw-empty-state icon="models" heading="No models yet" text="Search Hugging Face for a GGUF model; Pitwall tells you which quantisation fits this machine.">
          <button type="button" class="btn primary sm" (click)="discover.emit()"><mat-icon svgIcon="pw:search" aria-hidden="true" />Discover models</button>
        </pw-empty-state>
      } @else {
        <div class="scroll">
          <table class="table">
            <thead>
              <tr>
                <th scope="col">Model</th>
                <th scope="col">Quant</th>
                <th scope="col" class="right">Params</th>
                <th scope="col" class="right">Context</th>
                <th scope="col" class="right">Size</th>
                <th scope="col">Capabilities</th>
                <th scope="col">Engine</th>
                <th scope="col">Release when idle</th>
                <th scope="col">Status</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              @for (m of store.library(); track m.id) {
                <tr [class.hot]="m.loaded">
                  <td class="model">
                    <span class="name">{{ m.name }}</span>
                    <span class="repo">{{ m.repo }} · used {{ when(m.lastUsedAt, now()) }}</span>
                  </td>
                  <td><span class="tag mono">{{ m.quant ?? '—' }}</span></td>
                  <td class="right num">{{ params(m.paramsB) }}</td>
                  <td class="right num">{{ ctx(m.contextMax) }}</td>
                  <td class="right num">{{ size(m.sizeBytes) }}</td>
                  <td>
                    <span class="caps">
                      @for (c of m.capabilities; track c) {
                        <span class="tag">{{ capability[c] }}</span>
                      }
                    </span>
                  </td>
                  <td>
                    @if (store.engineChoices().length) {
                      <label class="sr-only" [for]="'engine-' + m.id">Engine for {{ m.name }}</label>
                      <select class="select idle" [id]="'engine-' + m.id" [disabled]="m.loaded || store.isBusy()('model:' + m.id)" [matTooltip]="m.loaded ? 'Unload the model to change its engine' : ''" (change)="setEngine(m, $any($event.target).value)">
                        <option value="" [selected]="!m.engine">llama.cpp</option>
                        @for (e of store.engineChoices(); track e.id) {
                          <option [value]="e.id" [selected]="m.engine === e.id">{{ e.name }}</option>
                        }
                      </select>
                    } @else {
                      <span class="faint">llama.cpp</span>
                    }
                  </td>
                  <td>
                    <label class="sr-only" [for]="'idle-' + m.id">Release {{ m.name }} when idle</label>
                    <select class="select idle" [id]="'idle-' + m.id" [disabled]="m.pinned || store.isBusy()('model:' + m.id)" (change)="setIdle(m, $any($event.target).value)">
                      <option value="" [selected]="m.idleSecs === null">Global setting</option>
                      @for (s of idleOptions(m.idleSecs); track s) {
                        <option [value]="s" [selected]="m.idleSecs === s">After {{ secs(s) }}</option>
                      }
                    </select>
                  </td>
                  <td>
                    @if (m.loaded) {
                      <span class="pill dot" data-tone="ok">Loaded</span>
                    } @else {
                      <span class="pill dot" [attr.data-tone]="fit(m).tone" [matTooltip]="m.fit.message">{{ fit(m).label }}</span>
                    }
                    @if (m.pinned) {
                      <mat-icon class="pinned" svgIcon="pw:pin" aria-label="Pinned" matTooltip="Pinned: never released automatically" />
                    }
                  </td>
                  <td class="right">
                    <span class="actions">
                      @if (m.loaded) {
                        <button type="button" class="btn sm" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.unloadModel(m.id)" [attr.aria-label]="'Unload ' + m.name">Unload</button>
                      } @else {
                        <button type="button" class="btn sm" [disabled]="store.isBusy()('model:' + m.id) || m.fit.verdict === 'wontFit'" (click)="store.loadModel(m.id)" [attr.aria-label]="'Load ' + m.name">Load</button>
                      }
                      <button type="button" class="btn ghost icon sm" [attr.aria-label]="(m.pinned ? 'Unpin ' : 'Pin ') + m.name" [matTooltip]="m.pinned ? 'Unpin' : 'Pin (keep loaded)'" [disabled]="store.isBusy()('model:' + m.id)" (click)="store.pinModel(m.id, !m.pinned)">
                        <mat-icon [svgIcon]="m.pinned ? 'pw:unpin' : 'pw:pin'" aria-hidden="true" />
                      </button>
                      <button type="button" class="btn ghost icon sm del" [attr.aria-label]="'Delete ' + m.name" matTooltip="Delete from disk" [disabled]="store.isBusy()('model:' + m.id)" (click)="remove(m)">
                        <mat-icon svgIcon="pw:trash" aria-hidden="true" />
                      </button>
                    </span>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </div>
      }
    </pw-panel>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--sp-3);
    }
    .disk {
      display: flex;
      gap: 2px;
      height: 8px;
      border-radius: var(--r-full);
      overflow: hidden;
      .seg {
        flex-basis: 0;
        min-width: 3px;
      }
    }
    [data-k='models'] {
      background: var(--accent);
    }
    [data-k='engines'] {
      background: color-mix(in srgb, var(--accent) 50%, var(--text-3));
    }
    [data-k='partial'] {
      background: var(--warn);
    }
    [data-k='free'] {
      background: var(--track);
    }
    .disk-legend {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-2) var(--sp-5);
      margin: var(--sp-3) 0 0;
      padding: 0;
      list-style: none;
      font-size: var(--fs-sm);
      color: var(--text-2);
      li {
        display: inline-flex;
        align-items: center;
        gap: 6px;
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
      }
      b {
        font-weight: var(--fw-medium);
        color: var(--text);
      }
      .unused {
        margin-left: auto;
        color: var(--warn);
      }
    }
    .scroll {
      overflow-x: auto;
      margin-top: var(--sp-2);
    }
    .table th:first-child,
    .table td:first-child {
      padding-left: var(--sp-4);
    }
    .table th:last-child,
    .table td:last-child {
      padding-right: var(--sp-4);
    }
    .table td {
      height: 52px;
    }
    .model {
      display: flex;
      flex-direction: column;
      justify-content: center;
      max-width: 300px;
    }
    .name {
      font-weight: var(--fw-medium);
    }
    .repo {
      font-size: var(--fs-xs);
      color: var(--text-3);
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .caps {
      display: inline-flex;
      gap: 4px;
    }
    .idle {
      height: 26px;
      font-size: var(--fs-sm);
    }
    .pinned {
      width: 13px;
      height: 13px;
      margin-left: 6px;
      vertical-align: -2px;
      color: var(--text-3);
    }
    .actions {
      display: inline-flex;
      align-items: center;
      gap: 2px;
      .btn.sm:not(.icon) {
        min-width: 62px;
        margin-right: var(--sp-1);
      }
    }
    .del:hover:not(:disabled) {
      color: var(--bad);
    }
  `,
})
export class LibraryTab {
  readonly now = input.required<number>();
  readonly discover = output<void>();
  protected readonly store = inject(ModelsStore);
  private readonly confirm = inject(Confirm);
  protected readonly size = bytes;
  protected readonly when = ago;
  protected readonly secs = seconds;
  protected readonly params = paramsText;
  protected readonly ctx = contextText;
  protected readonly idleOptions = idleOptions;
  protected readonly capability = CAPABILITY_LABEL;
  protected readonly fit = (m: LocalModel) => fitLook(m.fit.verdict);

  protected readonly summary = computed(() => {
    const lib = this.store.library();
    if (!lib.length) return '';
    const loaded = lib.filter((m) => m.loaded).length;
    return `${lib.length} models · ${loaded} loaded`;
  });

  protected readonly segments = computed(() => {
    const d = this.store.disk();
    if (!d) return [];
    return [
      { key: 'models', bytes: d.modelsBytes },
      { key: 'engines', bytes: d.enginesBytes },
      { key: 'partial', bytes: d.partialBytes },
      { key: 'free', bytes: d.freeBytes },
    ].filter((s) => s.bytes > 0);
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

  protected setIdle(m: LocalModel, value: string): void {
    void this.store.setIdle(m.id, value === '' ? null : Number(value));
  }

  protected setEngine(m: LocalModel, value: string): void {
    void this.store.setEngine(m.id, value === '' ? null : value);
  }

  protected async remove(m: LocalModel): Promise<void> {
    const ok = await this.confirm.ask({ title: `Delete ${m.name}?`, message: `${m.file} (${bytes(m.sizeBytes)}) is removed from disk. You can download it again later.`, confirm: 'Delete', danger: true });
    if (ok) await this.store.deleteModel(m.id);
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
