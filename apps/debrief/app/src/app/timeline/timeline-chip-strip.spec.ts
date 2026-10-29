import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it, vi } from 'vitest';
import { layoutStrip, TimelineChipStrip } from './timeline-chip-strip';
import { StripChip, TimelineChips } from './timeline-chips';

const chip = (id: string, x: number, w = 100): StripChip => ({ id, x, w, text: id, title: id, label: id, testid: 'c', accent: 'red', open: () => undefined });

describe('layoutStrip', () => {
  it('puts chips that do not touch on one row, in left-to-right order', () => {
    const { shown, more } = layoutStrip([chip('b', 300), chip('a', 10)], 1000);
    expect(shown.map((p) => [p.chip.id, p.row])).toEqual([['a', 0], ['b', 0]]);
    expect(more).toEqual([]);
  });

  it('shows two rows at most and folds the rest into the overflow, keeping the order', () => {
    const chips = ['a', 'b', 'c', 'd', 'e'].map((id, i) => chip(id, 100 + i * 10));
    const { shown, more } = layoutStrip(chips, 1000);
    expect(shown.map((p) => [p.chip.id, p.row])).toEqual([['a', 0], ['b', 1]]);
    expect(more.map((c) => c.id)).toEqual(['c', 'd', 'e']);
  });

  it('keeps room at the right end for the overflow chip only when something overflows', () => {
    const edge = layoutStrip([chip('a', 950)], 1000);
    expect(edge.shown.map((p) => p.flip)).toEqual([true]);
    const over = layoutStrip([chip('a', 0), chip('b', 5), chip('c', 10)], 1000);
    expect(over.more.map((c) => c.id)).toEqual(['c']);
  });
});

describe('TimelineChipStrip', () => {
  it('shows two rows and a "+N" menu whose entries do what their chips do', async () => {
    const open = vi.fn();
    TestBed.configureTestingModule({ providers: [TimelineChips] });
    const chips = TestBed.inject(TimelineChips);
    chips.register('x', signal(['a', 'b', 'c', 'd'].map((id, i) => ({ ...chip(id, 10 + i), label: `Open ${id}`, open: () => open(id) }))));
    const fixture = TestBed.createComponent(TimelineChipStrip);
    fixture.detectChanges();
    const el = fixture.nativeElement as HTMLElement;
    expect([...el.querySelectorAll('button.chip:not(.more)')].map((b) => b.getAttribute('aria-label'))).toEqual(['Open a', 'Open b']);
    const more = el.querySelector<HTMLButtonElement>('[data-testid=chip-more]')!;
    expect(more.textContent?.trim()).toBe('+2');
    expect(more.getAttribute('aria-label')).toBe('2 more captures, dumps and notes');
    more.click();
    fixture.detectChanges();
    await fixture.whenStable();
    const items = [...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')];
    expect(items.map((i) => i.getAttribute('aria-label'))).toEqual(['Open c', 'Open d']);
    items[1]!.click();
    expect(open).toHaveBeenCalledWith('d');
  });

  it('has no overflow chip when everything fits', () => {
    TestBed.configureTestingModule({ providers: [TimelineChips] });
    TestBed.inject(TimelineChips).register('x', signal([chip('a', 10), chip('b', 400)]));
    const fixture = TestBed.createComponent(TimelineChipStrip);
    fixture.detectChanges();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid=chip-more]')).toBeNull();
  });
});
