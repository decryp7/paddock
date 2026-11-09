import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import type { PopoverStatus } from '../../core/generated/widgets';

/**
 * The popup header's one status pill: how the local AI is doing overall, as the engine decided it ("Serving on :11435",
 * "Paused", "2 engine problems", "No answer"). An icon and a word, never colour alone; "Connecting…" until the first answer.
 */
@Component({
  selector: 'pw-status-pill',
  imports: [PdStatusGlyph],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span class="pill" [attr.data-tone]="look().tone" role="status"><pd-status-glyph [status]="look().glyph" [size]="14" />{{ look().text }}</span>
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
      white-space: nowrap;
    }
    .pill[data-tone='danger'] {
      border-color: transparent;
      background: var(--pd-danger-tint);
      color: var(--pd-danger);
    }
    .pill[data-tone='warning'] {
      border-color: transparent;
      background: var(--pd-warning-tint);
      color: var(--pd-warning);
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

  protected readonly look = computed<{ tone: 'danger' | 'warning' | 'success' | 'neutral'; glyph: PdStatus; text: string }>(() => {
    const s = this.status();
    switch (s?.kind) {
      case 'serving':
        return { tone: 'success', glyph: 'success', text: `Serving on :${s.port}` };
      case 'paused':
        return { tone: 'warning', glyph: 'warning', text: 'Paused' };
      case 'stopped':
        return { tone: 'warning', glyph: 'warning', text: 'Stopped' };
      case 'problems':
        return { tone: 'danger', glyph: 'error', text: `${s.count} engine ${s.count === 1 ? 'problem' : 'problems'}` };
      case 'unreachable':
        return { tone: 'danger', glyph: 'error', text: 'No answer' };
      default:
        return { tone: 'neutral', glyph: 'info', text: 'Connecting…' };
    }
  });
}
