import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink } from '@angular/router';
import type { LoadedModel } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { bytes, countdown } from '../../shared/format';
import { hotStateLook } from '../../shared/labels';
import { seriesColor } from '../../shared/theme-tokens';
import { EmptyState } from '../../shared/ui/empty-state';

/** How far a model is through its idle time (0 just used … 1 due), for the countdown bar. */
export function idleProgress(m: LoadedModel, now: number): number | null {
  if (m.idleReleaseAt === null) return null;
  const span = m.idleReleaseAt - m.lastUsedAt;
  if (span <= 0) return 1;
  return Math.max(0, Math.min(1, (now - m.lastUsedAt) / span));
}

/** The hot models with memory, state and a live countdown to auto-release; pin and unload in place. */
@Component({
  selector: 'pw-hot-models',
  imports: [MatIconModule, MatTooltipModule, RouterLink, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.models().length === 0) {
      <pw-empty-state icon="models" heading="No model loaded" [text]="compact() ? '' : 'Models load on the first request and are released when idle.'">
        @if (!compact()) {
          <a class="btn sm" routerLink="/models">Open the library</a>
        }
      </pw-empty-state>
    }
    <ul class="list" [class.compact]="compact()" aria-label="Loaded models">
      @for (m of live.models(); track m.modelId) {
        <li class="row">
          <div class="main">
            <div class="title">
              <span class="state" [attr.data-tone]="look(m).tone" [attr.aria-label]="look(m).label"></span>
              <span class="name">{{ m.name }}</span>
              @if (m.pinned) {
                <mat-icon class="pinned" svgIcon="pw:pin" aria-label="Pinned" matTooltip="Pinned: never released automatically" />
              }
            </div>
            <div class="meta num">
              @if (m.vramBytes) {
                <span>{{ size(m.vramBytes) }} VRAM</span>
              }
              <span>{{ size(m.ramBytes) }} RAM</span>
              @if (!compact()) {
                <span>{{ ctx(m.contextTokens) }} ctx</span>
              }
            </div>
          </div>
          <div class="side">
            @if (m.state === 'busy' || m.activeRequests > 0) {
              <span class="when busy">{{ m.activeRequests }} {{ m.activeRequests === 1 ? 'request' : 'requests' }}</span>
            } @else if (m.pinned) {
              <span class="when faint">Pinned</span>
            } @else if (m.idleReleaseAt !== null) {
              <span class="when num" [attr.aria-label]="'Released in ' + countdownText(m.idleReleaseAt, now())">
                <mat-icon svgIcon="pw:clock" aria-hidden="true" />{{ countdownText(m.idleReleaseAt, now()) }}
              </span>
              <span class="idle" aria-hidden="true"><span [style.width.%]="(progress(m, now()) ?? 0) * 100"></span></span>
            } @else {
              <span class="when faint">{{ look(m).label }}</span>
            }
          </div>
          @if (!compact()) {
            <div class="actions">
              <button type="button" class="btn ghost icon sm" [attr.aria-label]="(m.pinned ? 'Unpin ' : 'Pin ') + m.name" [matTooltip]="m.pinned ? 'Unpin' : 'Pin (keep loaded)'" [disabled]="live.isBusy()('model:' + m.modelId)" (click)="live.pinModel(m.modelId, !m.pinned)">
                <mat-icon [svgIcon]="m.pinned ? 'pw:unpin' : 'pw:pin'" aria-hidden="true" />
              </button>
              <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Unload ' + m.name" matTooltip="Unload now" [disabled]="live.isBusy()('model:' + m.modelId)" (click)="live.unloadModel(m.modelId)">
                <mat-icon svgIcon="pw:release" aria-hidden="true" />
              </button>
            </div>
          }
        </li>
      }
    </ul>
    @if (!compact() && split(); as s) {
      <div class="split">
        <div class="split-head"><span>GPU memory by model</span><span class="num faint">{{ size(s.used) }} of {{ size(s.total) }}</span></div>
        <div class="split-bar" role="img" [attr.aria-label]="s.label">
          @for (seg of s.segments; track seg.name) {
            <span [style.width.%]="seg.pct" [style.background]="seg.color" [matTooltip]="seg.name + ' · ' + size(seg.bytes)"></span>
          }
        </div>
        <ul class="split-legend" aria-hidden="true">
          @for (seg of s.segments; track seg.name) {
            <li><i [style.background]="seg.color"></i>{{ seg.name }}</li>
          }
        </ul>
      </div>
    }
  `,
  styles: `
    .split {
      margin-top: var(--sp-3);
      padding-top: var(--sp-3);
      border-top: 1px solid var(--border-soft);
    }
    .split-head {
      display: flex;
      justify-content: space-between;
      margin-bottom: var(--sp-2);
      font-size: var(--fs-xs);
      color: var(--text-2);
    }
    .split-bar {
      display: flex;
      gap: 2px;
      height: 8px;
      border-radius: var(--r-full);
      background: var(--track);
      overflow: hidden;
      span {
        height: 100%;
        transition: width var(--dur-slow) var(--ease);
      }
    }
    .split-legend {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-1) var(--sp-3);
      margin: var(--sp-2) 0 0;
      padding: 0;
      list-style: none;
      font-size: var(--fs-xs);
      color: var(--text-2);
      li {
        display: inline-flex;
        align-items: center;
        gap: 5px;
      }
      i {
        width: 8px;
        height: 8px;
        border-radius: 2px;
      }
    }
    :host {
      display: block;
    }
    .list {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
    }
    .row {
      display: grid;
      grid-template-columns: minmax(0, 1fr) auto auto;
      align-items: center;
      gap: var(--sp-3);
      padding: 10px 0;
      border-top: 1px solid var(--border-soft);
      &:first-child {
        border-top: none;
        padding-top: 2px;
      }
    }
    .compact .row {
      grid-template-columns: minmax(0, 1fr) auto;
      padding: 8px 0;
    }
    .title {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      min-width: 0;
    }
    .state {
      flex: none;
      width: 7px;
      height: 7px;
      border-radius: 50%;
      background: var(--text-3);
      &[data-tone='ok'] {
        background: var(--ok);
      }
      &[data-tone='info'] {
        background: var(--accent);
        box-shadow: 0 0 0 3px var(--accent-soft);
      }
      &[data-tone='warn'] {
        background: var(--warn);
      }
      &[data-tone='bad'] {
        background: var(--bad);
      }
    }
    .name {
      font-weight: var(--fw-medium);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .pinned {
      width: 13px;
      height: 13px;
      color: var(--text-3);
      flex: none;
    }
    .meta {
      display: flex;
      gap: var(--sp-2);
      margin: 2px 0 0 15px;
      font-size: var(--fs-xs);
      color: var(--text-3);
      white-space: nowrap;
      span + span::before {
        content: '·';
        margin-right: var(--sp-2);
      }
    }
    .side {
      display: flex;
      flex-direction: column;
      align-items: flex-end;
      gap: 5px;
      min-width: 64px;
    }
    .when {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      font-size: var(--fs-sm);
      font-weight: var(--fw-medium);
      white-space: nowrap;
      mat-icon {
        width: 12px;
        height: 12px;
        color: var(--text-3);
      }
      &.busy {
        color: var(--accent);
      }
    }
    .idle {
      display: block;
      width: 56px;
      height: 3px;
      border-radius: var(--r-full);
      background: var(--track);
      overflow: hidden;
      span {
        display: block;
        height: 100%;
        background: var(--warn);
        transition: width 1s linear;
      }
    }
    .actions {
      display: flex;
      gap: 2px;
    }
  `,
})
export class HotModels {
  readonly now = input.required<number>();
  readonly compact = input(false);
  protected readonly live = inject(LiveStore);
  protected readonly look = (m: LoadedModel) => hotStateLook(m.state);
  protected readonly size = bytes;
  protected readonly countdownText = countdown;
  protected readonly progress = idleProgress;
  private readonly theme = inject(ThemeService);

  /** VRAM split by hot model plus everything else on the GPU (other apps, the desktop). */
  protected readonly split = computed(() => {
    const g = this.live.memory()?.gpus[0];
    if (!g || !g.vramTotal || g.vramUsed === null || this.live.models().length === 0) return null;
    const total = g.vramTotal;
    const t = this.theme.tokens();
    const segments = this.live
      .models()
      .filter((m) => m.vramBytes > 0)
      .map((m, i) => ({ name: m.name, bytes: m.vramBytes, color: seriesColor(t, i) }));
    const models = segments.reduce((s, x) => s + x.bytes, 0);
    const other = Math.max(0, g.vramUsed - models);
    if (other > 0) segments.push({ name: 'Other apps', bytes: other, color: t.text3 });
    const withPct = segments.map((s) => ({ ...s, pct: (s.bytes / total) * 100 }));
    return { total, used: g.vramUsed, segments: withPct, label: `GPU memory: ${withPct.map((s) => `${s.name} ${Math.round(s.pct)}%`).join(', ')}` };
  });

  protected ctx(tokens: number): string {
    return tokens >= 1024 ? `${Math.round(tokens / 1024)}k` : String(tokens);
  }
}
