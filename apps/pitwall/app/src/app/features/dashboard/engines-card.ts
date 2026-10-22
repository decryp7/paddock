import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { EngineStatusTile } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { availabilityLook, type Look } from '../../shared/labels';

/** Engines at a glance: availability, active build and process health. */
@Component({
  selector: 'pw-engines-card',
  imports: [MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <ul aria-label="Engines">
      @for (e of engines(); track e.kind) {
        <li [class.off]="e.availability.state !== 'available'">
          <div class="main">
            <span class="name">{{ e.name }}</span>
            <span class="sub">
              @if (e.availability.state === 'available') {
                {{ e.activeBuild ?? 'no build' }} · {{ e.processes }} {{ e.processes === 1 ? 'process' : 'processes' }}
              } @else {
                {{ reason(e) }}
              }
            </span>
          </div>
          <span class="pill dot" [attr.data-tone]="look(e).tone" [matTooltip]="reason(e)">{{ look(e).label }}</span>
        </li>
      }
    </ul>
  `,
  styles: `
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
    }
    li {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-3);
      padding: 8px 0;
      border-top: 1px solid var(--border-soft);
      &:first-child {
        border-top: none;
        padding-top: 2px;
      }
    }
    .main {
      display: flex;
      flex-direction: column;
      min-width: 0;
    }
    .name {
      font-weight: var(--fw-medium);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .off .name {
      color: var(--text-2);
    }
    .sub {
      font-size: var(--fs-xs);
      color: var(--text-3);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
  `,
})
export class EnginesCard {
  private readonly live = inject(LiveStore);
  protected readonly engines = computed(() => this.live.overview()?.engines ?? []);

  protected look(e: EngineStatusTile): Look {
    if (e.availability.state === 'available' && e.unhealthy > 0) return { label: `${e.unhealthy} unhealthy`, tone: 'bad' };
    return availabilityLook(e.availability);
  }

  protected reason(e: EngineStatusTile): string {
    return e.availability.state === 'available' ? '' : e.availability.reason;
  }
}
