import { classifyScroll, ScrollRead } from './scroll-classify';

// ADR 0072, decision 2: a scroll event is the grid's own write (or its clamped echo), or the user's.
describe('classifyScroll', () => {
  const read = (patch: Partial<ScrollRead>): ScrollRead => ({ read: 1000, pendingWrite: null, lastWrite: Number.NaN, maxScroll: 5000, recentInput: true, layoutChanged: false, ...patch });

  const table: [string, Partial<ScrollRead>, 'own' | 'user' | 'spurious'][] = [
    ['the read of our last write', { read: 1000, lastWrite: 1000 }, 'own'],
    ['within 1 px of our last write (rounding)', { read: 1000.8, lastWrite: 1000 }, 'own'],
    ['more than 1 px from our last write', { read: 1003, lastWrite: 1000 }, 'user'],
    ['no write yet', { read: 1000, lastWrite: Number.NaN }, 'user'],
    ['a write clamped at the maximum (the spacer had not grown): the read is the maximum, the write was beyond it', { read: 5000, lastWrite: 5024, maxScroll: 5000 }, 'own'],
    ['a pending write clamped at the maximum', { read: 5000, pendingWrite: 5048, lastWrite: Number.NaN, maxScroll: 5000 }, 'own'],
    ['at the maximum but the write was not beyond it: the user scrolled there', { read: 5000, lastWrite: 1000, maxScroll: 5000 }, 'user'],
    ['at the maximum with a write beyond it only by rounding (not a clamp)', { read: 5000, lastWrite: 5000.4, maxScroll: 5000 }, 'own'],
    ['a read that is not a number', { read: Number.NaN, lastWrite: 1000 }, 'user'],
    ['the browser reset a far position to 0 while the layout changed, with no input: spurious', { read: 0, lastWrite: 120022, maxScroll: 119963, recentInput: false, layoutChanged: true }, 'spurious'],
    ['the browser shifted the position while the layout changed, with no input: spurious', { read: 119468, lastWrite: 119304, maxScroll: 120366, recentInput: false, layoutChanged: true }, 'spurious'],
    ['the same position change right after an input is the user', { read: 119468, lastWrite: 119304, maxScroll: 120366, recentInput: true, layoutChanged: true }, 'user'],
    ['a scroll with no input and no layout change is still the user (find in page, assistive technology)', { read: 3000, lastWrite: 120022, maxScroll: 119963, recentInput: false, layoutChanged: false }, 'user'],
    ['an infinite maximum (not laid out yet): only an equal read is ours', { read: 10, lastWrite: 5000, maxScroll: Number.POSITIVE_INFINITY }, 'user'],
  ];

  it.each(table)('%s', (_name, patch, expected) => {
    expect(classifyScroll(read(patch))).toBe(expected);
  });
});
