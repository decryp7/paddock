import { ChangeDetectionStrategy, Component, computed, input, signal } from '@angular/core';
import { splitForEllipsis } from '@decryp7/paddock-ui/format';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { copyText } from './clipboard';

/**
 * A path, file name or other long identifier on one line that shortens in the middle when its container is narrow (rule 18):
 * the start stays, an ellipsis replaces the rest of the start, and the file name (or the last characters) always stays
 * visible. The full value is the tooltip and, with `copy`, one click on a Copy button (shown on hover and focus). Fills the
 * width it is given: put it in a flex or grid child with `min-width: 0`.
 */
@Component({
  selector: 'pd-path-text',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `<span class="pd-text" [attr.title]="whole()" data-testid="path-text"><span class="pd-head">{{ parts().head }}</span><span class="pd-tail">{{ parts().tail }}</span></span>
    @if (copy()) {
      <button class="pd-copy" type="button" [attr.aria-label]="copied() ? copiedLabel : copyLabel" [attr.title]="copied() ? copiedLabel : copyLabel" (click)="doCopy($event)">
        <pd-glyph [name]="copied() ? 'check' : 'copy'" [size]="12" [stroke]="2.25" />
      </button>
    }`,
  styles: `
    :host { display: flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; }
    .pd-text { display: flex; min-width: 0; flex: 0 1 auto; white-space: nowrap; }
    .pd-head { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .pd-tail { flex: 0 0.001 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .pd-copy { flex: none; display: inline-flex; padding: 2px; border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text-muted); cursor: pointer; opacity: 0; }
    :host(:hover) .pd-copy, .pd-copy:focus-visible { opacity: 1; }
    .pd-copy:hover { color: var(--pd-text); background: var(--pd-hover); }
    .pd-copy:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    @media (hover: none) { .pd-copy { opacity: 0.6; } }
  `,
})
export class PdPathText {
  /** The text shown (shortened in the middle when it does not fit). */
  readonly value = input.required<string>();
  /** What the tooltip shows and Copy copies when it is more than `value` (a file name shown for its full path). */
  readonly full = input<string | null>(null);
  /** Characters of the end that are never cut (a longer file name keeps its last ones). */
  readonly keepTail = input(20);
  /** Shows a Copy button for the full value. */
  readonly copy = input(false);

  protected readonly parts = computed(() => splitForEllipsis(this.value(), this.keepTail()));
  protected readonly whole = computed(() => this.full() ?? this.value());
  protected readonly copied = signal(false);
  protected readonly copyLabel = $localize`:@@pd.pathText.copy:Copy the full path`;
  protected readonly copiedLabel = $localize`:@@pd.pathText.copied:Copied`;

  protected async doCopy(event: Event): Promise<void> {
    event.stopPropagation();
    const text = this.whole();
    if (!(await copyText(text))) return;
    this.copied.set(true);
    // Resetting the label is a side effect on this component's own display, a timer outside the signal graph.
    setTimeout(() => this.copied.set(false), 1500);
  }
}
