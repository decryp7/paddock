import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import type { Hint, MemoryArray } from '../../core/api.types';
import { formatBytes } from '../../shared/format';
import { describeSlots, slotModel, speedLabel } from './slot-model';

const W = 96;
const H = 236;

/**
 * The memory slots drawn as sockets on a board: filled slots hold a stick coloured by size (with its size, type and speed
 * on it), empty slots are dashed outlines, a slot a hint points at is highlighted, and dual-channel pairs are bracketed.
 * Colour is never the only signal: sizes are printed on every stick, and the text alternative lists every slot.
 */
@Component({
  selector: 'dy-memory-slots',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <figure>
      <svg [attr.viewBox]="'0 0 ' + width() + ' ' + (H + 44)" role="img" [attr.aria-label]="alt()" preserveAspectRatio="xMidYMid meet" [style.max-width.px]="width() * 1.7">
        <defs>
          <linearGradient id="dy-pcb" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="var(--surface-3)" /><stop offset="1" stop-color="var(--surface-inset)" /></linearGradient>
        </defs>
        <rect class="board" x="0.5" y="22.5" [attr.width]="width() - 1" [attr.height]="H + 20" rx="14" />
        @for (p of model().pairs; track p[0]) {
          <g class="pair">
            <path [attr.d]="bracket(p)" />
            <text [attr.x]="(x(p[0]) + x(p[1]) + 70) / 2" y="14" text-anchor="middle">Dual channel</text>
          </g>
        }
        @for (s of model().slots; track s.index) {
          <g [attr.transform]="'translate(' + x(s.index) + ' 34)'" [class.hinted]="!!s.hint" [class.filled]="!!s.module">
            <rect class="socket" x="0" y="0" width="70" [attr.height]="H" rx="8" />
            <rect class="latch" x="26" y="-4" width="18" height="6" rx="3" />
            @if (s.module; as m) {
              <g class="stick" [style.--c]="'var(--chart-' + (s.color + 1) + ')'">
                <rect class="body" x="6" y="8" width="58" [attr.height]="H - 36" rx="5" />
                @for (c of chips; track c) {
                  <rect class="chip" x="14" [attr.y]="22 + c * 28" width="42" height="18" rx="3" />
                }
                <path class="notch" d="M26 190 h18 v10 h-18 z" />
                <rect class="plate" x="10" y="62" width="50" height="34" rx="6" />
                <text class="size" x="35" y="85" text-anchor="middle">{{ size(m.capacityBytes) }}</text>
                <rect class="plate" x="10" y="118" width="50" height="22" rx="6" />
                <text class="speed" x="35" y="133" text-anchor="middle">{{ speed(m) }}</text>
                <path class="gold" d="M10 202 h50 v8 h-50 z" />
              </g>
            } @else {
              <text class="plus" x="35" y="104" text-anchor="middle">+</text>
              <text class="empty" x="35" y="128" text-anchor="middle">Empty</text>
            }
            <text class="slot" x="35" [attr.y]="H + 22" text-anchor="middle">{{ s.name }}</text>
          </g>
        }
      </svg>
      <figcaption class="visually-hidden">{{ alt() }}</figcaption>
    </figure>
  `,
  styles: `
    :host { display: block; }
    figure { margin: 0; display: grid; justify-items: center; }
    svg { width: 100%; height: auto; display: block; font-family: var(--font-sans); }
    .board { fill: url(#dy-pcb); stroke: var(--border-strong); }
    .socket { fill: var(--surface-1); stroke: var(--border-strong); stroke-width: 1; }
    :host-context(:root[data-theme='dark']) .socket { fill: #0b0c0e; }
    .latch { fill: var(--surface-active); stroke: var(--border-strong); }
    .stick .body { fill: var(--c); fill-opacity: 0.2; stroke: var(--c); stroke-width: 1.5; }
    .stick .chip { fill: var(--c); fill-opacity: 0.55; }
    .stick .notch { fill: var(--surface-1); }
    .stick .gold { fill: #c9a24a; }
    .plate { fill: var(--surface-2); fill-opacity: 0.92; stroke: var(--border); }
    .size { font-size: 17px; font-weight: 600; fill: var(--text-1); font-variant-numeric: tabular-nums; }
    .speed { font-size: 9px; fill: var(--text-2); font-variant-numeric: tabular-nums; }
    .slot { font-size: 11px; fill: var(--text-3); }
    .plus { font-size: 30px; fill: var(--text-3); font-weight: 300; }
    .empty { font-size: 12px; fill: var(--text-3); }
    .hinted .socket { stroke: var(--accent); stroke-width: 1.75; stroke-dasharray: 5 4; }
    .hinted.filled .socket { stroke: var(--warn); stroke-dasharray: none; }
    .pair path { fill: none; stroke: var(--ok); stroke-width: 1.5; }
    .pair text { font-size: 10.5px; fill: var(--ok); font-weight: 600; }
  `,
})
export class MemorySlots {
  readonly array = input.required<MemoryArray>();
  readonly hints = input<readonly Hint[]>([]);
  protected readonly H = H;
  protected readonly chips = [0, 1, 2, 3, 4];
  protected readonly model = computed(() => slotModel(this.array(), this.hints()));
  protected readonly width = computed(() => Math.max(1, this.model().slots.length) * W - (W - 70) + 24);
  protected readonly alt = computed(() => describeSlots(this.model()));
  protected readonly size = formatBytes;
  protected readonly speed = speedLabel;
  protected readonly x = (i: number) => 12 + i * W;
  protected bracket(p: [number, number]): string {
    const a = this.x(p[0]) + 35;
    const b = this.x(p[1]) + 35;
    return `M${a.toString()} 30 v-8 h${(b - a).toString()} v8`;
  }
}
