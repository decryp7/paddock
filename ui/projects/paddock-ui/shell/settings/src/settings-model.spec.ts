import { defineSettingsPage, msg } from '@decryp7/paddock-ui/shell';
import { SETTINGS_OVERVIEW_FIXTURE } from '../generated/settings-fixtures';
import type { FieldKind, SettingField } from '../generated/settings';
import {
  displayValue,
  editUnit,
  fieldText,
  formatBytes,
  formatMs,
  matchesQuery,
  parseList,
  parseNumber,
  precheck,
  rangeHint,
  sameValue,
  settingsNav,
  withinOneEdit,
  words,
} from './settings-model';

const field = (kind: FieldKind, extra: Partial<SettingField> = {}): SettingField => ({
  key: 'k',
  label: 'K',
  description: '',
  kind,
  default: '',
  restartRequired: false,
  advanced: false,
  ...extra,
});

describe('settings model', () => {
  it('writes durations and sizes in their largest whole unit, as Rust does', () => {
    expect([formatMs(0), formatMs(1500), formatMs(90_000), formatMs(120_000), formatMs(7_200_000)]).toEqual(['0 ms', '1500 ms', '90 s', '2 min', '2 h']);
    expect([formatBytes(1000), formatBytes(2048), formatBytes(10 << 20), formatBytes(3 * 2 ** 30)]).toEqual(['1000 bytes', '2 KB', '10 MB', '3 GB']);
  });

  it('edits durations and sizes in the unit both limits are multiples of', () => {
    expect(editUnit({ type: 'duration', minMs: 60_000, maxMs: 86_400_000 })).toEqual({ factor: 60_000, label: 'min' });
    expect(editUnit({ type: 'duration', minMs: 0, maxMs: 1500 })).toEqual({ factor: 1, label: 'ms' });
    expect(editUnit({ type: 'size', minBytes: 1 << 20, maxBytes: 1 << 30 })).toEqual({ factor: 1 << 20, label: 'MB' });
    expect(editUnit({ type: 'bool' })).toBeNull();
  });

  it('hints at every kind’s limits', () => {
    expect(rangeHint({ type: 'int', min: 1, max: 10, step: 1, unit: 'times' })).toBe('1–10 times');
    expect(rangeHint({ type: 'float', min: 0, max: 1, step: 0.1, unit: null })).toBe('0–1');
    expect(rangeHint({ type: 'duration', minMs: 60_000, maxMs: 86_400_000 })).toBe('1 min – 24 h');
    expect(rangeHint({ type: 'size', minBytes: 1 << 20, maxBytes: 1 << 30 })).toBe('1 MB – 1 GB');
    expect(rangeHint({ type: 'text', maxLen: 40, pattern: null })).toBe('up to 40 characters');
    expect(rangeHint({ type: 'stringList', maxItems: 20 })).toBe('up to 20 items, one per line');
    expect([rangeHint({ type: 'path', dir: true }), rangeHint({ type: 'path', dir: false })]).toEqual(['folder', 'file']);
    expect(rangeHint({ type: 'bool' })).toBe('');
  });

  it('shows values as the user reads them', () => {
    expect(displayValue(field({ type: 'bool' }), true)).toBe('on');
    expect(displayValue(field({ type: 'bool' }), false)).toBe('off');
    expect(displayValue(field({ type: 'duration', minMs: 0, maxMs: 1 }), 3_600_000)).toBe('1 h');
    expect(displayValue(field({ type: 'size', minBytes: 0, maxBytes: 1 }), 64 << 20)).toBe('64 MB');
    expect(displayValue(field({ type: 'enum', options: [{ value: 'a', label: 'Alpha' }] }), 'a')).toBe('Alpha');
    expect(displayValue(field({ type: 'enum', options: [] }), 'z')).toBe('z');
    expect(displayValue(field({ type: 'stringList', maxItems: 3 }), ['a', 'b'])).toBe('a, b');
    expect(displayValue(field({ type: 'stringList', maxItems: 3 }), [])).toBe('none');
    expect(displayValue(field({ type: 'int', min: 0, max: 9, step: 1, unit: 'times' }), 3)).toBe('3 times');
    expect(displayValue(field({ type: 'text', maxLen: 9, pattern: null }), '')).toBe('not set');
    expect(displayValue(field({ type: 'text', maxLen: 9, pattern: null }), 'x')).toBe('x');
    expect(sameValue(['a'], ['a']) && !sameValue(1, '1')).toBe(true);
  });

  it('pre-checks types and ranges only', () => {
    const int = field({ type: 'int', min: 1, max: 10, step: 1, unit: null });
    expect([precheck(int, 5), precheck(int, 0), precheck(int, 2.5), precheck(int, 'x')]).toEqual([null, 'Between 1–10.', 'Enter a whole number.', 'Enter a number.']);
    const float = field({ type: 'float', min: 0, max: 1, step: 0.1, unit: null });
    expect([precheck(float, 0.5), precheck(float, 2)]).toEqual([null, 'Between 0–1.']);
    const dur = field({ type: 'duration', minMs: 1000, maxMs: 2000 });
    expect([precheck(dur, 1500), precheck(dur, 10), precheck(dur, 'x')]).toEqual([null, 'Between 1 s – 2 s.', 'Enter a number.']);
    const size = field({ type: 'size', minBytes: 1024, maxBytes: 2048 });
    expect(precheck(size, 4096)).toBe('Between 1 KB – 2 KB.');
    expect(precheck(field({ type: 'text', maxLen: 2, pattern: null }), 'abc')).toBe('At most 2 characters.');
    expect(precheck(field({ type: 'text', maxLen: 2, pattern: '[a]+' }), 'b')).toBeNull();
    expect(precheck(field({ type: 'stringList', maxItems: 1 }), ['a', 'b'])).toBe('At most 1 items.');
    expect(precheck(field({ type: 'bool' }), 'x')).toBeNull();
  });

  it('parses numbers in their edit unit and lists by line', () => {
    expect(parseNumber('2', 60_000)).toBe(120_000);
    expect(parseNumber('0.1', 3)).toBe(0.3);
    expect(parseNumber(' ')).toBe(' ');
    expect(parseNumber('abc')).toBe('abc');
    expect(parseList('a\r\n\n b \n')).toEqual(['a', 'b']);
  });

  it('finds identifiers however they are written (rule 20)', () => {
    expect(words('logFolder data_dir pd.cache-size')).toEqual(['log', 'folder', 'data', 'dir', 'pd', 'cache', 'size']);
    expect(withinOneEdit('folder', 'folder')).toBe(true);
    expect(withinOneEdit('folder', 'fodler')).toBe(true);
    expect(withinOneEdit('folder', 'folders')).toBe(true);
    expect(withinOneEdit('folders', 'folder')).toBe(true);
    expect(withinOneEdit('folder', 'filder')).toBe(true);
    expect(withinOneEdit('folder', 'fdlore')).toBe(false);
    expect(withinOneEdit('a', 'abc')).toBe(false);
    const text = 'Data folder Empty: the app’s folder. folder sd.storage';
    for (const q of ['data folder', 'datafolder', 'DataFolder', 'data_folder', 'fold', 'fodler', 'DATA']) expect(matchesQuery(text, q), q).toBe(true);
    for (const q of ['', '  ', 'greeting', 'fdlore', 'xyz']) expect(matchesQuery(text, q), q).toBe(false);
  });

  it('searches a field by its label, help, key, unit, choices and section', () => {
    const general = SETTINGS_OVERVIEW_FIXTURE.sections.find((s) => s.id === 'sd.general')!;
    const repeat = general.fields.find((f) => f.key === 'repeat')!;
    expect(fieldText(general, repeat)).toContain('times');
    const style = general.fields.find((f) => f.key === 'style')!;
    expect(matchesQuery(fieldText(general, style), 'formal')).toBe(true);
    expect(matchesQuery(fieldText(general, style), 'general')).toBe(true);
  });

  it('lays out the navigation by group and order, with unlisted sections under Advanced', () => {
    const load = () => Promise.resolve(class {});
    const pages = [
      defineSettingsPage({ id: 'sd.storage', group: 'features', title: msg('sd.storage', 'Storage'), order: 100, load }),
      defineSettingsPage({ id: 'sd.general', group: 'application', title: msg('sd.general', 'General'), order: 100, load }),
      defineSettingsPage({ id: 'pd.appearance', group: 'application', title: msg('pd.appearance', 'Appearance'), order: 50, load }),
      defineSettingsPage({ id: 'sd.keys', group: 'application', title: msg('sd.keys', 'Keys'), order: 100, load }),
    ];
    const extra = { ...SETTINGS_OVERVIEW_FIXTURE.sections[1]!, id: 'sd.hidden', title: 'Hidden' };
    const nav = settingsNav(pages, [...SETTINGS_OVERVIEW_FIXTURE.sections, extra]);
    expect(nav.map((g) => [g.title, g.items.map((i) => i.id)])).toEqual([
      ['Application', ['pd.appearance', 'sd.general', 'sd.keys']],
      ['Features', ['sd.storage']],
      ['Advanced', ['sd.hidden']],
    ]);
    expect(nav[0]!.items[2]!.section).toBeNull();
    expect(nav[2]!.items[0]!.page).toBeNull();
    expect(settingsNav([], [])).toEqual([]);
  });
});
