import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { PopoverStatus } from '../../core/generated/widgets';

/**
 * The popup header's one status pill: how the watches are doing overall, as the engine decided it ("2 failing", "All good",
 * "3 not checked yet"). An icon and a word, never colour alone; nothing when there is nothing to say. What is new is said by the
 * Recent changes widget's head, not here.
 */
@Component({
  selector: 'sp-status-pill',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (look(); as l) {
      <span class="pill" [attr.data-tone]="l.tone" role="status"><mat-icon [svgIcon]="l.icon" aria-hidden="true" />{{ l.text }}</span>
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .pill {
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-1);
      margin-left: auto;
      padding: 1px var(--pd-space-2);
      border-radius: var(--pd-radius-pill);
      background: var(--pd-surface-3);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
    }
    .pill[data-tone='danger'] {
      background: var(--pd-danger-tint);
      color: var(--pd-danger);
    }
    .pill[data-tone='success'] {
      background: var(--pd-success-tint);
      color: var(--pd-success);
    }
    .pill mat-icon {
      width: 12px;
      height: 12px;
    }
  `,
})
export class StatusPill {
  readonly status = input<PopoverStatus | null>(null);

  protected readonly look = computed<{ tone: 'danger' | 'success' | 'neutral'; icon: string; text: string } | null>(() => {
    const s = this.status();
    switch (s?.kind) {
      case 'failing':
        return { tone: 'danger', icon: 'sp:alert', text: `${s.count} failing` };
      case 'allGood':
        return { tone: 'success', icon: 'sp:check', text: 'All good' };
      case 'unchecked':
        return { tone: 'neutral', icon: 'sp:clock', text: `${s.count} not checked yet` };
      default:
        return null;
    }
  });
}
