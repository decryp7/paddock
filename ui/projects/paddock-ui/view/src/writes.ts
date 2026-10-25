import { ChangeDetectionStrategy, Component } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';

/** The text a `writes` action carries so nobody changes something without noticing (`ActionRef.writes`). */
export const writesText = (): string => $localize`:@@pd.view.writes:Changes data`;

/**
 * The one "Changes data" badge (pencil glyph and the words): on a button, a link, the reason-send button of a review row and the
 * host's confirmation, so an action that writes always looks the same. Put it inside the control it describes.
 */
@Component({
  selector: 'pd-view-writes',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  host: { 'data-testid': 'view-writes' },
  template: `<pd-glyph name="edit" [size]="14" /><span class="pd-view-writes__text" [attr.data-short]="short">{{ text }}</span>`,
  styles: `
    :host { display: inline-flex; align-items: center; gap: var(--pd-space-1); margin-inline-start: var(--pd-space-2); padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-pill); background: var(--pd-surface-1); color: var(--pd-text); border: 1px solid var(--pd-border-strong); font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); line-height: 1.5; white-space: nowrap; }
    /* In a narrow pane the badge is the pencil and a shorter word (the full words stay in the page for assistive technology). */
    @container (max-width: 360px) {
      .pd-view-writes__text { font-size: 0; }
      .pd-view-writes__text::after { content: attr(data-short); font-size: var(--pd-text-caption); }
    }
  `,
})
export class PdViewWrites {
  protected readonly text = writesText();
  protected readonly short = $localize`:@@pd.view.writes.short:Edits`;
}
