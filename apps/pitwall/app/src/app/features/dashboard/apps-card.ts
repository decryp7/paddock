import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatTooltipModule } from '@angular/material/tooltip';
import { RouterLink } from '@angular/router';
import type { AppInfo, AppLimits, KeyedSeries } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ThemeService } from '../../core/theme';
import { compact, duration, int } from '../../shared/format';
import { seriesColor } from '../../shared/theme-tokens';
import { EmptyState } from '../../shared/ui/empty-state';
import { Sparkline } from '../../shared/ui/sparkline';

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
  imports: [MatTooltipModule, RouterLink, Sparkline, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (live.apps().length === 0) {
      <pw-empty-state icon="apps" heading="No apps yet" text="Apps pair with Pitwall the first time they ask for a model, or you create a key for them.">
        <a class="btn sm" routerLink="/apps">Apps & keys</a>
      </pw-empty-state>
    } @else {
      <table class="table">
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
                  <span class="nm">{{ a.name }}</span>
                  <span class="conn" [class.on]="a.connected" [attr.aria-label]="a.connected ? 'Connected' : 'Not connected'" [matTooltip]="a.connected ? 'Connected' : 'Not connected'"></span>
                </span>
              </td>
              <td class="right num">{{ intText(a.today.requests) }}</td>
              <td class="right num">{{ count(a.today.promptTokens + a.today.completionTokens) }}</td>
              <td class="right num">{{ dur(a.today.busyMs) }}</td>
              <td class="right num" [class.bad]="a.today.errors > 0">{{ a.today.errors }}</td>
              <td class="limits"><span class="lim" [matTooltip]="limits(a.permissions.limits)">{{ limits(a.permissions.limits) }}</span></td>
              <td class="trend"><pw-sparkline [values]="trend(a)" [color]="color(a)" [width]="80" [height]="22" /></td>
            </tr>
          }
        </tbody>
      </table>
    }
  `,
  styles: `
    :host {
      display: block;
      margin: 0 calc(-1 * var(--sp-4)) calc(-1 * var(--sp-2));
    }
    .table th:first-child,
    .table td:first-child {
      padding-left: var(--sp-4);
    }
    .table th:last-child,
    .table td:last-child {
      padding-right: var(--sp-4);
    }
    .app {
      display: inline-flex;
      align-items: center;
      gap: var(--sp-2);
    }
    .avatar {
      display: grid;
      place-items: center;
      width: 20px;
      height: 20px;
      font-size: var(--fs-2xs);
      font-weight: var(--fw-semibold);
      color: var(--text);
      background: color-mix(in srgb, var(--c) 24%, transparent);
      box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--c) 45%, transparent);
      border-radius: var(--r-sm);
    }
    .nm {
      font-weight: var(--fw-medium);
    }
    .conn {
      width: 6px;
      height: 6px;
      border-radius: 50%;
      border: 1.5px solid var(--text-3);
      &.on {
        border-color: var(--ok);
        background: var(--ok);
      }
    }
    .bad {
      color: var(--bad);
      font-weight: var(--fw-medium);
    }
    .limits {
      color: var(--text-2);
      font-size: var(--fs-sm);
    }
    .lim {
      display: block;
      max-width: 110px;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .trend {
      width: 96px;
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
