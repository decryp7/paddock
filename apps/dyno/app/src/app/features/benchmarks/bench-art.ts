import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { HW_TOKENS } from '../inventory/hardware-tokens';

/**
 * Flat illustrations for the two benchmark kinds the Inventory art does not cover: a memory module and a graphics card,
 * in the same materials (green board, graphite packages, gold contacts, aluminium). Decorative.
 */
@Component({
  selector: 'dy-bench-art',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (kind() === 'memory') {
      <svg viewBox="0 0 240 70" aria-hidden="true" focusable="false">
        <rect class="pcb" x="0.5" y="0.5" width="239" height="56" rx="4" />
        <path class="notch" d="M112 56 a6 6 0 0 0 12 0" />
        @for (i of chips; track i) {
          <rect class="chip" [attr.x]="14 + i * 54" y="10" width="44" height="30" rx="2" />
          <path class="mark" [attr.d]="'M' + (20 + i * 54) + ' 20H' + (52 + i * 54) + 'M' + (20 + i * 54) + ' 28H' + (44 + i * 54)" />
        }
        @for (i of pins; track i) {
          <rect class="gold" [attr.x]="4 + i * 6" y="57" width="3.4" height="11" [attr.opacity]="i > 17 && i < 21 ? 0 : 1" />
        }
      </svg>
    } @else {
      <svg viewBox="0 0 240 90" aria-hidden="true" focusable="false">
        <rect class="pcb" x="0.5" y="0.5" width="239" height="70" rx="5" />
        <rect class="sink" x="22.5" y="8.5" width="150" height="54" rx="4" />
        @for (i of fins; track i) {
          <path class="fin" [attr.d]="'M' + (34 + i * 10) + ' 14V56'" />
        }
        <circle class="fan" cx="206" cy="35" r="24" />
        <circle class="hub" cx="206" cy="35" r="6" />
        <path class="blade" d="M206 11V29M206 59V41M182 35H200M230 35H212" />
        @for (i of pins; track i) {
          <rect class="gold" [attr.x]="12 + i * 7" y="71" width="4" height="12" [attr.opacity]="i > 8 && i < 11 ? 0 : 1" />
        }
      </svg>
    }
  `,
  styles: [
    HW_TOKENS,
    `
    :host { display: block; }
    svg { width: 100%; height: auto; display: block; }
    .pcb { fill: var(--hw-pcb-b); stroke: var(--hw-pcb-edge); }
    .notch { fill: none; stroke: var(--hw-pcb-edge); }
    .chip { fill: var(--hw-pkg-b); stroke: var(--hw-pkg-edge); }
    .mark { fill: none; stroke: var(--hw-pkg-mark); stroke-opacity: 0.45; shape-rendering: crispEdges; }
    .gold { fill: var(--hw-gold-a); }
    .sink { fill: var(--hw-metal-b); stroke: var(--hw-metal-edge); }
    .fin { stroke: var(--hw-metal-edge); stroke-opacity: 0.7; stroke-width: 1.5; }
    .fan { fill: var(--hw-pkg-b); stroke: var(--hw-pkg-edge); stroke-width: 2; }
    .hub { fill: var(--hw-metal-a); stroke: var(--hw-metal-edge); }
    .blade { stroke: var(--hw-pkg-mark); stroke-width: 4; stroke-linecap: round; stroke-opacity: 0.6; }
  `,
  ],
})
export class BenchArt {
  readonly kind = input.required<'memory' | 'gpu'>();
  protected readonly chips = [0, 1, 2, 3];
  protected readonly pins = Array.from({ length: 38 }, (_, i) => i);
  protected readonly fins = Array.from({ length: 14 }, (_, i) => i);
}
