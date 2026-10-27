import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/** A stable hue (0-359) for a key: the same app always gets the same tile colour. FNV-1a, so near names still differ. */
export function avatarHue(key: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < key.length; i++) {
    h ^= key.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0) % 360;
}

/** The identity of an app: its first letter on a rounded square whose colour comes from its id (decorative; the name is always beside it). */
@Component({
  selector: 'pw-app-avatar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true', '[class.lg]': 'size() === "lg"', '[style.--h]': 'hue()' },
  template: `{{ letter() }}`,
  styles: `
    :host {
      display: inline-grid;
      flex: none;
      place-items: center;
      width: 20px;
      height: 20px;
      font-size: var(--fs-xs);
      font-weight: var(--fw-semibold);
      line-height: 1;
      color: var(--text);
      background: hsl(var(--h) 70% 55% / 0.24);
      box-shadow: inset 0 0 0 1px hsl(var(--h) 70% 55% / 0.5);
      border-radius: var(--r-sm);
    }
    :host(.lg) {
      width: 40px;
      height: 40px;
      font-size: var(--fs-lg);
      border-radius: var(--r-lg);
    }
  `,
})
export class AppAvatar {
  /** The app's id (or name): the colour comes from it. */
  readonly key = input.required<string>();
  readonly name = input.required<string>();
  readonly size = input<'sm' | 'lg'>('sm');
  protected readonly hue = computed(() => avatarHue(this.key()));
  protected readonly letter = computed(() => (this.name().trim()[0] ?? '?').toUpperCase());
}
