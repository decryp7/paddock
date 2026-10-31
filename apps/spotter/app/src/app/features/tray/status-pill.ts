import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import type { PopoverStatus } from '../../core/generated/widgets';

/**
 * The popup header's one status pill: how the watches are doing overall, as the engine decided it ("2 failing", "All good",
 * "3 not checked yet"). An icon and a word, never colour alone; nothing when there is nothing to say. What is new is said by the
 * Recent changes widget's head, not here.
 */
@Component({
  selector: 'sp-status-pill',
  imports: [PdStatusGlyph],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (look(); as l) {
      <span class="pill" [attr.data-tone]="l.tone" role="status"><pd-status-glyph [status]="l.glyph" [size]="14" />{{ l.text }}</span>
    }
  `,
  styles: `
    :host {
      display: contents;
    }
    .pill {
      border: 1px solid transparent;
      display: inline-flex;
      align-items: center;
      gap: var(--pd-space-1);
      margin-left: auto;
      padding: 1px var(--pd-space-2);
      border-radius: var(--pd-radius-pill);
      /* Neutral: lighter than the popover and outlined, so it shows on the popover's surface. */
      background: var(--pd-surface-1);
      border-color: var(--pd-border-strong);
      color: var(--pd-text-muted);
      font-size: var(--pd-text-caption);
      font-weight: var(--pd-weight-semibold);
    }
    .pill[data-tone='danger'] {
      border-color: transparent;
      background: var(--pd-danger-tint);
      color: var(--pd-danger);
    }
    .pill[data-tone='success'] {
      border-color: transparent;
      background: var(--pd-success-tint);
      color: var(--pd-success);
    }
  `,
})
export class StatusPill {
  readonly status = input<PopoverStatus | null>(null);

  protected readonly look = computed<{ tone: 'danger' | 'success' | 'neutral'; glyph: PdStatus; text: string } | null>(() => {
    const s = this.status();
    switch (s?.kind) {
      case 'failing':
        return { tone: 'danger', glyph: 'warning', text: `${s.count} failing` };
      case 'allGood':
        return { tone: 'success', glyph: 'success', text: 'All good' };
      case 'unchecked':
        return { tone: 'neutral', glyph: 'info', text: `${s.count} not checked yet` };
      default:
        return null;
    }
  });
}
