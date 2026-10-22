import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { Router } from '@angular/router';
import { formatBytes, formatCount, formatPercent } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { LiveStore } from '../../state/live.store';
import type { AppGroup } from '../../core/api.types';

/** The apps using the most CPU or memory right now, grouped by app ("Chrome · 38"); a click opens them in the task manager. */
@Component({
  selector: 'dy-top-apps',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" [attr.aria-label]="'Top apps by ' + metric()">
      <div class="card-head">
        <h2><dy-icon [name]="metric() === 'cpu' ? 'cpu' : 'memory'" /> Top apps by {{ metric() === 'cpu' ? 'CPU' : 'memory' }}</h2>
        <span class="spacer"></span>
        <button type="button" class="btn link" (click)="openAll()">View all</button>
      </div>
      @if (!live.connected()) {
        <div class="skeletons" aria-busy="true" aria-label="Loading apps">
          @for (i of placeholders; track i) {
            <div class="skeleton"></div>
          }
        </div>
      } @else {
        <ol>
          @for (g of rows(); track g.id) {
            <li>
              <button type="button" (click)="open(g)" [attr.aria-label]="'Show ' + g.name + ' in Processes'">
                <span class="name truncate">{{ g.name }}</span>
                @if (g.pids.length > 1) {
                  <span class="mult num" [title]="count(g.pids.length) + ' processes'">×{{ count(g.pids.length) }}</span>
                }
                <span class="val num">{{ value(g) }}</span>
                <span class="bar-track" aria-hidden="true"><span [style.width.%]="share(g)"></span></span>
              </button>
            </li>
          }
        </ol>
      }
    </section>
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .card { height: 100%; padding-bottom: var(--sp-2); }
    h2 { white-space: nowrap; }
    .skeletons { display: grid; gap: var(--sp-3); padding: var(--sp-4); }
    .skeleton { height: 22px; }
    ol { list-style: none; margin: 0; padding: var(--sp-2) var(--sp-2) 0; display: grid; gap: 1px; }
    button { display: grid; grid-template-columns: minmax(0, 1fr) auto auto; align-items: center; column-gap: var(--sp-2); width: 100%; padding: 7px var(--sp-2) 8px; border: 0; border-radius: var(--r-md); background: none; color: var(--text-1); font: inherit; text-align: left; cursor: default; row-gap: 5px; }
    button:hover { background: var(--surface-hover); }
    .bar-track { grid-column: 1 / -1; height: 4px; }
    .val { font-weight: var(--fw-medium); min-width: 56px; text-align: right; }
    .name { font-weight: var(--fw-medium); }
    .mult { color: var(--text-3); font-size: var(--fs-xs); }
  `,
})
export class TopApps {
  protected readonly live = inject(LiveStore);
  private readonly router = inject(Router);
  readonly metric = input<'cpu' | 'memory'>('cpu');
  protected readonly count = formatCount;
  protected readonly placeholders = [1, 2, 3, 4, 5];

  protected readonly rows = computed<AppGroup[]>(() => {
    const u = this.live.update();
    return ((this.metric() === 'cpu' ? u?.topCpu : u?.topMemory) ?? []).slice(0, 7);
  });
  private readonly max = computed(() => Math.max(1, ...this.rows().map((g) => this.raw(g))));

  private raw(g: AppGroup): number {
    return this.metric() === 'cpu' ? g.cpu : g.memory;
  }
  protected value(g: AppGroup): string {
    return this.metric() === 'cpu' ? formatPercent(g.cpu) : formatBytes(g.memory);
  }
  protected share(g: AppGroup): number {
    return (this.raw(g) / this.max()) * 100;
  }
  protected open(g: AppGroup): void {
    void this.router.navigate(['/processes'], { queryParams: { q: g.name } });
  }
  protected openAll(): void {
    void this.router.navigate(['/processes']);
  }
}
