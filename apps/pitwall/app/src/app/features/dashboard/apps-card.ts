import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink } from '@angular/router';
import type { AppInfo, AppLimits, KeyedSeries } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { compact, duration, int } from '../../shared/format';
import { seriesColor } from '../../shared/theme-tokens';
import { MatButton } from '@angular/material/button';
import { MatIcon } from '@angular/material/icon';
import { SuEmptyState, SuSparkline } from '@decryp7/paddock-ui';

/** The limits that are set, in short (`600/min · 5M tok/day`), or `No limits`. */
export function limitsText(l: AppLimits): string {
  const parts: string[] = [];
  if (l.requestsPerMin !== null) parts.push(`${l.requestsPerMin}/min`);
  if (l.requestsPerDay !== null) parts.push(`${compact(l.requestsPerDay)} req/day`);
  if (l.tokensPerDay !== null) parts.push(`${compact(l.tokensPerDay)}/day`);
  if (l.tokensPerMonth !== null) parts.push(`${compact(l.tokensPerMonth)}/mo`);
  if (l.maxMemoryPercent !== null) parts.push(`≤${l.maxMemoryPercent}% RAM`);
  if (l.maxConcurrent !== null) parts.push(`×${l.maxConcurrent}`);
  return parts.length ? parts.join(' · ') : 'None';
}

/** Connected apps with today's requests, tokens, busy time, errors and limits, and a trend over the chart range. */
@Component({
  selector: 'pw-apps-card',
  imports: [MatTooltipModule, MatButton, MatIcon, RouterLink, SuSparkline, SuEmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.apps().length === 0) {
      <pd-empty-state heading="No apps yet" description="Apps pair with Pitwall the first time they ask for a model, or you create a key for them.">
        <mat-icon slot="icon" svgIcon="pw:apps" aria-hidden="true" />
        <a slot="action" matButton="outlined" routerLink="/apps">Apps & keys</a>
      </pd-empty-state>
    } @else {
      <table class="table fit">
        <colgroup><col /><col class="c-n" /><col class="c-n" /><col class="c-n" /><col class="c-n" /><col class="c-lim" /><col class="c-trend" /></colgroup>
        <thead>
          <tr>
            <th scope="col">App</th>
            <th scope="col" class="right">Requests</th>
            <th scope="col" class="right">Tokens</th>
            <th scope="col" class="right">Busy</th>
            <th scope="col" class="right">Errors</th>
            <th scope="col">Limits</th>
            <th scope="col"><span class="sr-only">Trend</span></th>
          </tr>
        </thead>
        <tbody>
          @for (a of apps(); track a.id) {
            <tr>
              <td>
                <span class="app">
                  <span class="avatar" [style.--c]="color(a)" aria-hidden="true">{{ a.name[0] }}</span>
                  <span class="nm" [matTooltip]="a.name" matTooltipShowDelay="600">{{ a.name }}</span>
                  <span class="conn" role="img" [class.on]="a.connected" [attr.aria-label]="a.connected ? 'Connected' : 'Not connected'" [matTooltip]="a.connected ? 'Connected' : 'Not connected'"></span>
                </span>
              </td>
              <td class="right num">{{ intText(a.today.requests) }}</td>
              <td class="right num">{{ count(a.today.promptTokens + a.today.completionTokens) }}</td>
              <td class="right num">{{ dur(a.today.busyMs) }}</td>
              <td class="right num" [class.bad]="a.today.errors > 0">{{ a.today.errors }}</td>
              <td class="limits"><span class="lim" [matTooltip]="limits(a.permissions.limits)">{{ limits(a.permissions.limits) }}</span></td>
              <td class="trend"><pd-sparkline [values]="trend(a)" [min]="0" [height]="22" [label]="'Requests of ' + a.name" [style.color]="color(a)" /></td>
            </tr>
          }
        </tbody>
      </table>
    }
  `,
  styles: `
    :host {
      display: block;
      margin: 0 calc(-1 * var(--pd-space-4)) calc(-1 * var(--pd-space-2));
    }
    .c-n {
      width: 72px;
    }
    .c-lim {
      width: 96px;
    }
    .trend pd-sparkline {
      width: 64px;
    }
    .c-trend {
      width: 88px;
    }
    .table th:first-child,
    .table td:first-child {
      padding-left: var(--pd-space-4);
    }
    .table th:last-child,
    .table td:last-child {
      padding-right: var(--pd-space-4);
    }
    .app {
      display: inline-flex;
      max-width: 100%;
      align-items: center;
      gap: var(--pd-space-2);
      > * {
        flex: none;
      }
    }
    .avatar {
      display: grid;
      place-items: center;
      width: 20px;
      height: 20px;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-semibold);
      color: var(--pd-text);
      background: color-mix(in srgb, var(--c) 24%, transparent);
      box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 45%, transparent);
      border-radius: var(--pd-radius-tag);
    }
    .app > .nm {
      flex: 0 1 auto;
      min-width: 0;
      overflow: hidden;
      font-weight: var(--pd-weight-medium);
      text-overflow: ellipsis;
    }
    .conn {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      border: 1.5px solid var(--pd-text-subtle);
      &.on {
        border-color: var(--pd-success);
        background: var(--pd-success);
      }
    }
    .bad {
      color: var(--pd-danger);
      font-weight: var(--pd-weight-medium);
    }
    .limits {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-label);
    }
    .lim {
      display: block;
      overflow: hidden;
      text-overflow: ellipsis;
    }
  `,
})
export class AppsCard {
  /** Requests per step per app id over the chart range. */
  readonly series = input<KeyedSeries[]>([]);
  protected readonly live = inject(LiveStore);
  private readonly theme = inject(ThemeService);
  protected readonly intText = int;
  protected readonly count = compact;
  protected readonly dur = duration;
  protected readonly limits = limitsText;
  protected readonly apps = computed(() => [...this.live.apps()].sort((a, b) => b.today.requests - a.today.requests));
  private readonly byKey = computed(() => new Map(this.series().map((s) => [s.key, s.values])));

  /** Colour follows the app (its slot in the chart's series), never its rank in the table. */
  protected color(a: AppInfo): string {
    const keys = this.series().map((s) => s.key);
    const i = keys.indexOf(a.id);
    return seriesColor(this.theme.tokens(), i >= 0 ? i : keys.length + this.live.apps().findIndex((x) => x.id === a.id));
  }

  protected trend(a: AppInfo): number[] {
    return this.byKey().get(a.id) ?? [];
  }
}
