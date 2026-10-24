import { describe, expect, it } from 'vitest';
import type { Node } from '../generated/view';
import { ICON_GLYPH, KNOWN_KINDS, TONE_GLYPH, childKey, glyphFor, isWrite, mergeArgs, shapeOf, toneOf, toneWord } from './model';

describe('shapeOf', () => {
  it('reads each known kind with its payload', () => {
    const n: Node = { id: 'x', kind: { badge: { text: 'B', tone: 'good' } } };
    expect(shapeOf(n)).toEqual({ tag: 'badge', v: { text: 'B', tone: 'good' } });
    expect(KNOWN_KINDS).toContain('reviewList');
  });
  it('never throws: anything else is unknown', () => {
    expect(shapeOf({ id: '', kind: { hologram: {} } } as unknown as Node)).toEqual({ tag: 'unknown', name: 'hologram' });
    expect(shapeOf({ id: '', kind: {} } as unknown as Node)).toEqual({ tag: 'unknown', name: '' });
    expect(shapeOf({ id: '', kind: null } as unknown as Node)).toEqual({ tag: 'unknown', name: '' });
    expect(shapeOf({ id: '', kind: { text: 5 } } as unknown as Node)).toEqual({ tag: 'unknown', name: 'text' });
    expect(shapeOf(null as unknown as Node)).toEqual({ tag: 'unknown', name: '' });
  });
});

describe('tones and icons', () => {
  it('maps a tone to its shape and word, and an unknown tone to neutral', () => {
    expect(toneOf('bad')).toBe('bad');
    expect(toneOf('purple')).toBe('neutral');
    expect(toneOf(undefined)).toBe('neutral');
    expect(new Set(['info', 'good', 'warn', 'bad'].map((t) => TONE_GLYPH[t as 'info']))).toHaveProperty('size', 4);
    expect(toneWord('neutral')).toBe('');
    expect(['info', 'good', 'warn', 'bad'].map((t) => toneWord(t as 'info')).every(Boolean)).toBe(true);
  });
  it('gives every icon of the closed enum a glyph and a fallback to anything else', () => {
    expect(Object.keys(ICON_GLYPH)).toHaveLength(14);
    expect(glyphFor('lock')).toBe('lock');
    expect(glyphFor('rocket')).toBe('info');
    expect(glyphFor(null)).toBe('info');
  });
});

describe('mergeArgs', () => {
  it('adds the host keys to object arguments and keeps the plugin keys', () => {
    expect(mergeArgs({ a: 1 }, { row: 2 })).toEqual({ a: 1, row: 2 });
    expect(mergeArgs({ row: 1 }, { row: 2 })).toEqual({ row: 2 });
    expect(mergeArgs(null, { page: 3 })).toEqual({ page: 3 });
    expect(mergeArgs(undefined, { page: 3 })).toEqual({ page: 3 });
  });
  it('passes arguments through when there is nothing to add, and wraps non-objects', () => {
    expect(mergeArgs({ a: 1 }, {})).toEqual({ a: 1 });
    expect(mergeArgs(undefined, {})).toBeNull();
    expect(mergeArgs('x', { row: 1 })).toEqual({ args: 'x', row: 1 });
    expect(mergeArgs([1], { row: 1 })).toEqual({ args: [1], row: 1 });
  });
  it('helpers', () => {
    expect(isWrite({ id: 'a', label: 'a', confirm: null, args: null, writes: true })).toBe(true);
    expect(isWrite(null)).toBe(false);
    expect(childKey({ id: 'x', kind: { badge: { text: 'a', tone: 'neutral' } } }, 3)).toBe('x');
    expect(childKey({ id: '', kind: { badge: { text: 'a', tone: 'neutral' } } }, 3)).toBe('#3');
  });
});
