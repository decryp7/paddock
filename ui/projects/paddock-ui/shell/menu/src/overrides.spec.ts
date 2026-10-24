import { defineKeybinding } from '@decryp7/paddock-ui/shell';
import {
  conflictsOf,
  declaredKeys,
  describeConflict,
  effectiveBindings,
  MAX_OVERRIDES,
  occupantsOf,
  sanitizeOverrides,
  withOverride,
  withoutOverride,
} from './overrides';

const DECLARED = [
  defineKeybinding({ command: 'sd.greet', key: 'mod+shift+h' }),
  defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' }),
  defineKeybinding({ command: 'sd.reset', key: 'Mod+R', scope: 'page.home' }),
  defineKeybinding({ command: 'sd.bad', key: 'Hyper+Q' }),
];

describe('effective bindings', () => {
  it('canonicalises declared keys, once each, and drops malformed ones', () => {
    expect(effectiveBindings(DECLARED, {})).toEqual([
      { command: 'sd.greet', key: 'Mod+Shift+H', scope: 'window', source: 'default' },
      { command: 'sd.reset', key: 'Mod+R', scope: 'page.home', source: 'default' },
    ]);
    expect(declaredKeys(DECLARED, 'sd.greet')).toEqual(['Mod+Shift+H', 'Mod+Shift+H']);
    expect(declaredKeys(DECLARED, 'sd.none')).toEqual([]);
  });

  it('replaces a command’s keys with the user’s, in its scope; null removes them', () => {
    expect(effectiveBindings(DECLARED, { 'sd.reset': 'Mod+Alt+R', 'sd.greet': null, 'sd.new': 'F7' })).toEqual([
      { command: 'sd.new', key: 'F7', scope: 'window', source: 'user' },
      { command: 'sd.reset', key: 'Mod+Alt+R', scope: 'page.home', source: 'user' },
    ]);
    expect(effectiveBindings([], { 'sd.x': 'not a key' })).toEqual([]);
  });

  it('sets, clears and drops overrides that equal the declared key', () => {
    expect(withOverride({}, 'sd.greet', 'mod+g', ['Mod+Shift+H'])).toEqual({ 'sd.greet': 'Mod+G' });
    expect(withOverride({ 'sd.greet': 'Mod+G' }, 'sd.greet', 'Mod+Shift+H', ['Mod+Shift+H'])).toEqual({});
    expect(withOverride({}, 'sd.greet', null, ['Mod+Shift+H'])).toEqual({ 'sd.greet': null });
    expect(withOverride({ 'sd.x': 'F2' }, 'sd.x', null, [])).toEqual({});
    expect(withOverride({ 'sd.x': 'F2' }, 'sd.x', 'Hyper+K', [])).toEqual({});
    expect(withOverride({}, 'sd.two', 'Mod+A', ['Mod+A', 'Mod+B'])).toEqual({ 'sd.two': 'Mod+A' });
    expect(withoutOverride({ a: 'F1', b: null }, 'a')).toEqual({ b: null });
  });
});

describe('sanitizeOverrides', () => {
  it('keeps valid entries and drops the rest', () => {
    expect(sanitizeOverrides({ 'sd.a': 'mod+k', 'sd.b': null, 'sd.c': 'Hyper+K', 'sd.d': 5, '': 'F1', 'a b': 'F1' })).toEqual({ 'sd.a': 'Mod+K', 'sd.b': null });
    for (const bad of [null, 'x', [], 3]) expect(sanitizeOverrides(bad)).toEqual({});
  });

  it('is bounded and ignores a prototype key', () => {
    const many = Object.fromEntries(Array.from({ length: MAX_OVERRIDES + 50 }, (_, i) => [`sd.c${i}`, 'F1']));
    expect(Object.keys(sanitizeOverrides(many))).toHaveLength(MAX_OVERRIDES);
    const hostile = JSON.parse('{"__proto__": "F1", "sd.ok": "F2"}');
    expect(sanitizeOverrides(hostile)).toEqual({ 'sd.ok': 'F2' });
    expect(sanitizeOverrides({ ['x'.repeat(201)]: 'F1' })).toEqual({});
  });
});

describe('conflicts', () => {
  it('reports two commands on the same keys in one scope, per platform', () => {
    const bindings = effectiveBindings(
      [
        defineKeybinding({ command: 'sd.a', key: 'Mod+R' }),
        defineKeybinding({ command: 'sd.b', key: 'Ctrl+R' }),
        defineKeybinding({ command: 'sd.c', key: 'Mod+R', scope: 'page.home' }),
        defineKeybinding({ command: 'sd.d', key: 'F5' }),
      ],
      { 'sd.e': 'F5' },
    );
    const found = conflictsOf(bindings);
    expect(found.map((c) => [c.first.command, c.second.command, c.platforms])).toEqual([
      ['sd.d', 'sd.e', ['mac', 'other']],
      ['sd.a', 'sd.b', ['other']],
    ]);
    expect(describeConflict(found[0]!)).toBe('F5 (sd.d) and F5 (sd.e) both press F5 in scope window');
    const other = conflictsOf(effectiveBindings([defineKeybinding({ command: 'sd.a', key: 'Mod+R' }), defineKeybinding({ command: 'sd.b', key: 'Ctrl+R' })], {}));
    expect(other.map((c) => c.platforms)).toEqual([['other']]);
    expect(describeConflict(other[0]!)).toContain('on Windows and Linux');
    const mac = conflictsOf(effectiveBindings([defineKeybinding({ command: 'sd.a', key: 'Mod+R' }), defineKeybinding({ command: 'sd.b', key: 'Meta+R' })], {}));
    expect(describeConflict(mac[0]!)).toContain('on macOS');
  });

  it('lists what already takes a key, never the command itself', () => {
    const bindings = effectiveBindings(DECLARED, {});
    const taken = occupantsOf('Ctrl+Shift+H', 'window', 'sd.other', bindings, [{ key: 'Ctrl+Shift+H', description: 'Help' }, { key: 'F6', description: 'Regions' }]);
    expect(taken.commands.map((b) => b.command)).toEqual(['sd.greet']);
    expect(taken.reserved.map((r) => r.description)).toEqual(['Help']);
    expect(occupantsOf('Mod+Shift+H', 'window', 'sd.greet', bindings).commands).toEqual([]);
    expect(occupantsOf('Mod+R', 'window', 'sd.x', bindings).commands).toEqual([]);
  });
});
