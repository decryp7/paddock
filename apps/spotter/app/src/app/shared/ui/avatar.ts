import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { hueOf, monogram } from '../identity';

/** A watch's identity: a monogram on a rounded square whose colour comes from the name (decorative; the name is always next to it). */
@Component({
  selector: 'sp-avatar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[style.--h]': 'hue()', '[style.--s]': "size() + 'px'", '[class.lg]': 'size() >= 44', 'aria-hidden': 'true' },
  template: `{{ letters() }}`,
  styles: `
    :host {
      display: inline-grid;
      place-items: center;
      flex: none;
      width: var(--s);
      height: var(--s);
      border-radius: calc(var(--s) * 0.28);
      background: hsl(var(--h) 60% 50% / 0.16);
      box-shadow: inset 0 0 0 1px hsl(var(--h) 60% 50% / 0.22);
      color: color-mix(in srgb, var(--pd-text) 30%, hsl(var(--h) 65% 48%));
      font-size: calc(var(--s) * 0.38);
      font-weight: var(--pd-weight-semibold);
      letter-spacing: 0.02em;
      line-height: 1;
      user-select: none;
    }
  `,
})
export class Avatar {
  readonly name = input.required<string>();
  readonly size = input(32);
  protected readonly letters = computed(() => monogram(this.name()));
  protected readonly hue = computed(() => hueOf(this.name()));
}
