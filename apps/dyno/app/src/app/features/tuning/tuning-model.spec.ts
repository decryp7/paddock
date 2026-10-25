import type { TuningSuggestion } from '../../core/api.types';
import { demoTuning } from '../../core/mock/data';
import { CATEGORIES, actionFor, groupTuning, summary } from './tuning-model';

const base = demoTuning()[0]!;
const make = (over: Partial<TuningSuggestion>): TuningSuggestion => ({ ...base, applied: false, blocked: null, changedOutside: false, ...over });

describe('groupTuning', () => {
  it('groups in the order of the page, leaves empty groups out and puts applied changes first', () => {
    const groups = groupTuning([make({ id: 'a', category: 'appearance' }), make({ id: 'b', category: 'power' }), make({ id: 'c', category: 'appearance', applied: true })]);
    expect(groups.map((g) => g.id)).toEqual(['power', 'appearance']);
    expect(groups[1]!.items.map((i) => i.id)).toEqual(['c', 'a']);
  });
  it('knows every category the engine can send', () => {
    expect(CATEGORIES.map((c) => c.id)).toEqual(['power', 'sleep', 'battery', 'developer', 'appearance']);
    expect(groupTuning(demoTuning()).flatMap((g) => g.items).length).toBe(demoTuning().length);
    expect(groupTuning([])).toEqual([]);
  });
});

describe('actionFor', () => {
  it('chooses the button from the state', () => {
    expect(actionFor(make({}))).toBe('review');
    expect(actionFor(make({ blocked: 'Needs administrator rights' }))).toBe('blocked');
    expect(actionFor(make({ applied: true }))).toBe('revert');
    expect(actionFor(make({ applied: true, changedOutside: true }))).toBe('revertAnyway');
  });
});

describe('summary', () => {
  it('counts what is ready, applied and waiting', () => {
    expect(summary([])).toBe('Nothing to tune on this computer right now.');
    expect(summary([make({}), make({ blocked: 'x' }), make({ applied: true })])).toBe('1 ready to review · 1 applied by Dyno · 1 waiting for administrator rights');
    expect(summary([make({})])).toBe('1 ready to review');
  });
});
