import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { RouterLink } from '@angular/router';
import type { WatchNumbers, WatchSummary } from '../../core/api.types';
import { cadenceMarks, cadenceTotal } from '../../shared/cadence';
import { spanText } from '../../shared/format';
import { Avatar } from '../../shared/ui/avatar';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { Version } from '../../shared/ui/version';

interface Card {
  watch: WatchSummary;
  marks: { x: number; count: number }[];
  total: number;
  average: string | null;
}

/** One card per watch that released something in the range: a timeline of when its releases landed. */
@Component({
  selector: 'sp-cadence-cards',
  imports: [RouterLink, Avatar, Version, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="grid">
      @for (c of cards(); track c.watch.id) {
        <a class="card" [routerLink]="['/watches', c.watch.id]" [attr.aria-label]="c.watch.name + ', ' + c.total + ' releases in ' + rangeLabel()">
          <div class="top">
            <sp-avatar [name]="c.watch.name" [size]="28" />
            <div class="who">
              <span class="name truncate">{{ c.watch.name }}</span>
              <span class="sub">{{ c.total }} {{ c.total === 1 ? 'release' : 'releases' }} · {{ c.average ? 'about every ' + c.average : 'in ' + rangeLabel() }}</span>
            </div>
            @if (c.watch.latest) { <sp-ver class="latest" [value]="c.watch.latest.version" /> }
          </div>
          <svg class="line" viewBox="0 0 100 14" preserveAspectRatio="none" aria-hidden="true">
            <line x1="0" y1="7" x2="100" y2="7" class="axis" vector-effect="non-scaling-stroke" />
            @for (m of c.marks; track m.x) {
              <line [attr.x1]="m.x" [attr.x2]="m.x" y1="2" y2="12" class="tick" [class.multi]="m.count > 1" vector-effect="non-scaling-stroke" />
            }
          </svg>
          <div class="axis-labels"><span>{{ rangeLabel() }} ago</span><span>last {{ c.watch.latest ? (c.watch.latest.firstSeenMs | ago) : '—' }}</span></div>
        </a>
      }
    </div>
  `,
  styles: `
    .grid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
      gap: var(--pd-card-gap);
    }
    .card {
      display: grid;
      gap: var(--pd-space-2);
      padding: var(--pd-space-4);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      background: var(--pd-surface-1);
      color: var(--pd-text);
      text-decoration: none;
      transition: border-color var(--pd-duration-micro) var(--pd-ease-move), transform var(--pd-duration-micro) var(--pd-ease-move);
    }
    .card:hover {
      border-color: var(--pd-border-strong);
      transform: translateY(-1px);
    }
    .top {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
    }
    .who {
      display: grid;
      flex: 1;
      min-width: 0;
    }
    .name {
      font-weight: var(--pd-weight-semibold);
    }
    .sub,
    .axis-labels {
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
    }
    .latest {
      font-size: var(--pd-text-body);
    }
    .line {
      width: 100%;
      height: 20px;
      overflow: visible;
    }
    .axis {
      stroke: var(--pd-hairline);
      stroke-width: 2;
    }
    .tick {
      stroke: var(--pd-accent);
      stroke-width: 2.5;
      stroke-linecap: round;
    }
    .tick.multi {
      stroke-width: 4;
    }
    .axis-labels {
      display: flex;
      justify-content: space-between;
      font-size: 11px;
    }
  `,
})
export class CadenceCards {
  readonly watches = input.required<readonly WatchSummary[]>();
  readonly numbers = input.required<readonly WatchNumbers[]>();
  /** `30 days`, `90 days`, `year`. */
  readonly rangeLabel = input('90 days');

  protected readonly cards = computed<Card[]>(() => {
    const byId = new Map(this.watches().map((w) => [w.id, w]));
    return this.numbers()
      .flatMap((n) => {
        const watch = byId.get(n.watchId);
        const total = cadenceTotal(n.releases);
        return watch && total > 0 ? [{ watch, marks: cadenceMarks(n.releases), total, average: n.averageIntervalMs ? spanText(n.averageIntervalMs) : null }] : [];
      })
      .sort((a, b) => b.total - a.total || a.watch.name.localeCompare(b.watch.name));
  });
}
