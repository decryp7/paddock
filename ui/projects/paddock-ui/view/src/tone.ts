import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Tone } from '../generated/view';
import { TONE_GLYPH, toneOf, toneWord } from './model';

/** CSS for the tone classes (`pd-view-tone-good` ...) every view component includes: text colour and tint from the design tokens, never a colour of the plugin's. */
export const TONE_CSS = `
  .pd-view-tone-info { --pd-view-tone: var(--pd-info); --pd-view-tint: var(--pd-info-tint); }
  .pd-view-tone-good { --pd-view-tone: var(--pd-success); --pd-view-tint: var(--pd-success-tint); }
  .pd-view-tone-warn { --pd-view-tone: var(--pd-warning); --pd-view-tint: var(--pd-warning-tint); }
  .pd-view-tone-bad { --pd-view-tone: var(--pd-danger); --pd-view-tint: var(--pd-danger-tint); }
  .pd-view-tone-neutral { --pd-view-tone: var(--pd-text-muted); --pd-view-tint: var(--pd-neutral-tint); }
`;

/** The fill of a hovered row: a visible step over any surface (the family `--pd-hover` is only 5%, too faint on these small rows). */
export const VIEW_HOVER = 'color-mix(in srgb, var(--pd-text) 9%, transparent)';
/** The same step as an opaque fill for a drawn box (an SVG rect must hide the line behind it). */
export const VIEW_HOVER_FILL = 'color-mix(in srgb, var(--pd-surface-2) 86%, var(--pd-text))';

/** The class of a tone, for `[class]`. */
export const toneClass = (tone: unknown): string => `pd-view-tone-${toneOf(tone)}`;

/**
 * The mark of a tone: a glyph of its own shape (14 px) and the word for assistive technology, so a tone is never colour alone. A
 * neutral tone shows nothing.
 */
@Component({
  selector: 'pd-view-tone',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `@if (glyph()) {<pd-glyph [name]="glyph()" [size]="14" /><span class="pd-view-sr">{{ word() }}: </span>}`,
  styles: `
    :host { display: inline-flex; align-items: center; gap: var(--pd-space-1); flex: none; }
    :host:empty { display: none; }
    .pd-view-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
  `,
})
export class PdViewTone {
  readonly tone = input<Tone | string>('neutral');
  protected readonly glyph = computed(() => TONE_GLYPH[toneOf(this.tone())]);
  protected readonly word = computed(() => toneWord(toneOf(this.tone())));
}
