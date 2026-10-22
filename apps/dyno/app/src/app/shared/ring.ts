import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';

/**
 * A score ring (0–100): a track and an arc in the tone's colour, with the number in the middle. The arc animates
 * when the value changes (unless the user prefers reduced motion).
 */
@Component({
  selector: 'dy-ring',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'img', '[attr.aria-label]': 'label()' },
  template: `
    <svg viewBox="0 0 120 120" aria-hidden="true">
      <circle class="track" cx="60" cy="60" [attr.r]="r" />
      <circle class="arc" cx="60" cy="60" [attr.r]="r" [attr.data-tone]="tone()" [style.stroke-dasharray]="circumference" [style.stroke-dashoffset]="offset()" transform="rotate(-90 60 60)" />
    </svg>
    <div class="center"><ng-content /></div>
  `,
  styles: `
    :host { position: relative; display: inline-grid; place-items: center; width: var(--ring-size, 132px); aspect-ratio: 1; }
    svg { position: absolute; inset: 0; width: 100%; height: 100%; }
    circle { fill: none; stroke-width: 9; }
    .track { stroke: var(--surface-active); }
    .arc { stroke: var(--accent); stroke-linecap: round; transition: stroke-dashoffset var(--dur-slow) var(--ease-out); }
    .arc[data-tone='ok'] { stroke: var(--ok); }
    .arc[data-tone='warn'] { stroke: var(--warn); }
    .arc[data-tone='bad'] { stroke: var(--bad); }
    .center { position: relative; display: grid; place-items: center; text-align: center; }
  `,
})
export class Ring {
  readonly value = input.required<number>();
  readonly tone = input<'ok' | 'warn' | 'bad' | 'accent'>('accent');
  readonly label = input('');
  protected readonly r = 52;
  protected readonly circumference = 2 * Math.PI * 52;
  protected readonly offset = computed(() => this.circumference * (1 - Math.min(100, Math.max(0, this.value())) / 100));
}
