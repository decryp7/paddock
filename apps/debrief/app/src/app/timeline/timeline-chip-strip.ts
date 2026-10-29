import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { MatMenu, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { placeLabels } from './label-rows';
import { StripChip, TimelineChips } from './timeline-chips';

/** Rows of chips the strip shows; the rest fold into a "+N" menu. */
export const STRIP_ROWS = 2;
/** Width reserved at the strip's right end for the "+N" chip, px. */
export const MORE_WIDTH = 48;

/** A chip with the row it sits on and whether it hangs left of its anchor. */
export interface PlacedChip {
  chip: StripChip;
  row: number;
  flip: boolean;
}

/**
 * Lays the chips out in left-to-right order on at most `rows` rows within `widthPx`, keeping `MORE_WIDTH` free for the
 * "+N" chip when some do not fit; those that do not fit are returned apart, in the same order. Pure.
 */
export function layoutStrip(chips: readonly StripChip[], widthPx: number, rows = STRIP_ROWS): { shown: PlacedChip[]; more: StripChip[] } {
  const sorted = [...chips].sort((a, b) => a.x - b.x || a.id.localeCompare(b.id));
  const place = (room: number) => placeLabels(sorted.map((c) => ({ x: c.x, w: c.w })), room);
  let slots = place(widthPx);
  if (slots.some((s) => s.row >= rows)) slots = place(widthPx - MORE_WIDTH);
  const shown: PlacedChip[] = [];
  const more: StripChip[] = [];
  sorted.forEach((chip, i) => {
    const s = slots[i]!;
    if (s.row < rows) shown.push({ chip, row: s.row, flip: s.flip });
    else more.push(chip);
  });
  return { shown, more };
}

/**
 * The timeline's chip strip: a row of the sticky header, between the histogram and the time axis, that holds the
 * chips of captures, dumps and notes so they never cover lane markers, the histogram or the tick labels. Two rows at most;
 * the others are in the "+N" menu, where each entry does what its chip does.
 */
@Component({
  selector: 'debrief-timeline-chip-strip',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatMenu, MatMenuItem, MatMenuTrigger],
  template: `
    @for (p of laid().shown; track p.chip.id) {
      <button
        type="button"
        class="chip"
        [attr.data-testid]="p.chip.testid"
        [style.--c]="p.chip.accent"
        [style.left.px]="p.flip ? null : p.chip.x"
        [style.right.px]="p.flip ? width() - p.chip.x : null"
        [style.top.px]="2 + p.row * 18"
        [title]="p.chip.title"
        [attr.aria-label]="p.chip.label"
        (pointerdown)="$event.stopPropagation()"
        (click)="p.chip.open()"
      >{{ p.chip.text }}</button>
    }
    @if (laid().more.length; as n) {
      <button
        type="button"
        class="chip more"
        data-testid="chip-more"
        [matMenuTriggerFor]="menu"
        [attr.aria-label]="moreLabel(n)"
        [title]="moreLabel(n)"
        (pointerdown)="$event.stopPropagation()"
      >+{{ n }}</button>
      <mat-menu #menu="matMenu" class="pd-popmenu">
        @for (c of laid().more; track c.id) {
          <button type="button" mat-menu-item class="pd-popmenu__item" [attr.data-testid]="c.testid + '-item'" [attr.aria-label]="c.label" [title]="c.title" (click)="c.open()">{{ c.text }}</button>
        }
      </mat-menu>
    }
  `,
  styles: `
    :host { display: block; position: relative; height: var(--tl-strip-h); }
    .chip { pointer-events: auto; position: absolute; box-sizing: border-box; height: 16px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font: inherit; font-size: var(--fs-2xs); line-height: 14px; padding: 0 5px; border-radius: 3px; border: 1px solid var(--c, var(--border-3)); background: var(--bg-3); color: var(--text); cursor: pointer; }
    .chip:hover { background: var(--bg-hover); }
    .chip:focus-visible { outline: 2px solid var(--focus-ring); }
    .more { right: 0; top: 2px; font-weight: 600; }
  `,
})
export class TimelineChipStrip {
  /** Plot width in px. */
  readonly width = input(1000);
  /** Rows shown before the rest fold into the menu. */
  readonly rows = input(STRIP_ROWS);
  private readonly chips = inject(TimelineChips);
  protected readonly laid = computed(() => layoutStrip(this.chips.all(), this.width(), this.rows()));

  protected moreLabel(n: number): string {
    return $localize`:@@timeline.chipStrip.more:${n}:count: more captures, dumps and notes`;
  }
}
