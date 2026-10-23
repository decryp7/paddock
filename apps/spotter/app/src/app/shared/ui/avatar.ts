import { ChangeDetectionStrategy, Component, computed, effect, inject, input, linkedSignal } from '@angular/core';
import { AvatarsStore } from '../../state/avatars.store';
import { hueOf, monogram } from '../identity';

/**
 * A watch's identity (decorative; the name is always next to it): the image the engine cached for the watch (favicon or org
 * avatar) when `src` is given and loads, else a monogram on a rounded square whose colour comes from the name.
 */
@Component({
  selector: 'sp-avatar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[style.--h]': 'hue()', '[style.--s]': "size() + 'px'", '[class.lg]': 'size() >= 44', 'aria-hidden': 'true' },
  template: `
    @if (image(); as url) {
      <img [src]="url" alt="" draggable="false" (error)="broken.set(true)" />
    } @else {
      {{ letters() }}
    }
  `,
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
      overflow: hidden;
    }
    img {
      width: 100%;
      height: 100%;
      object-fit: cover;
    }
  `,
})
export class Avatar {
  readonly name = input.required<string>();
  readonly size = input(32);
  /** The cached image for this watch, or null for none (then the monogram shows). */
  readonly src = input<string | null>(null);
  /** A watch whose cached picture to show (looked up once through the avatars store). */
  readonly watch = input<string | null>(null);
  /** A catalogue preset whose picture to show. */
  readonly preset = input<string | null>(null);
  private readonly avatars = inject(AvatarsStore);
  private readonly looked = computed(() => {
    const w = this.watch();
    const p = this.preset();
    if (w) return this.avatars.urlOf({ watch: w });
    return p ? this.avatars.urlOf({ preset: p }) : null;
  });
  /** The image failed to load: back to the monogram. */
  protected readonly broken = linkedSignal<string | null, boolean>({ source: () => this.src() ?? this.looked(), computation: () => false });
  protected readonly image = computed(() => (this.broken() ? null : (this.src() ?? this.looked())));
  protected readonly letters = computed(() => monogram(this.name()));
  protected readonly hue = computed(() => hueOf(this.name()));

  constructor() {
    effect(() => {
      const w = this.watch();
      const p = this.preset();
      if (w) this.avatars.request({ watch: w });
      else if (p) this.avatars.request({ preset: p });
    });
  }
}
