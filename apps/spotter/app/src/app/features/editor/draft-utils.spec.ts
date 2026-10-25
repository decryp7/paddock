import type { FieldSpec, SourceKind } from '../../core/api.types';
import { SOURCE_KINDS } from '../../core/mock/mock-data';
import { blankDraft } from '../../core/draft';
import { EXTENSION_CHIPS, defaultValues, draftFor, extensionList, findAction, missingFields, parseArgs, patchProduct, replaceAction, suggestName, toggleAction, toggleExtension, toggleTracked } from './draft-utils';

const field = (key: string, over: Partial<FieldSpec> = {}): FieldSpec => ({
  key, label: key, help: '', kind: 'text', required: true, default: '', placeholder: '', min: null, max: null, options: [], advanced: false, ...over,
});
const kind = (fields: FieldSpec[]): SourceKind => ({ id: 'k', label: 'Kind label', description: '', group: 'files', fields } as SourceKind);

describe('draft utils', () => {
  it('collects only non-empty field defaults', () => {
    expect(defaultValues(kind([field('a', { default: 'x' }), field('b')]))).toEqual({ a: 'x' });
  });

  it('builds a fresh draft for a kind with the given interval, dropping the id', () => {
    const base = blankDraft({ id: 'old', name: 'Keep' });
    const d = draftFor(base, kind([field('a', { default: '1' })]), 900);
    expect(d).toMatchObject({ id: null, name: 'Keep', schedule: { mode: 'every', secs: 900 }, source: { kind: 'k', values: { a: '1' } } });
  });

  it('lists required non-secret fields without a value; blanks count as missing', () => {
    const k = kind([field('a'), field('b', { required: false }), field('c', { kind: 'secret' }), field('d')]);
    expect(missingFields(k, { a: '  ', d: 'ok' }).map((f) => f.key)).toEqual(['a']);
    expect(missingFields(null, {})).toEqual([]);
  });

  it('suggests a name from the first filled non-secret, non-toggle field', () => {
    const k = kind([field('tok', { kind: 'secret' }), field('on', { kind: 'toggle' }), field('url')]);
    expect(suggestName(k, { tok: 's', on: 'true', url: 'https://x.example/a/b-tool/?q=1#h' })).toBe('b-tool');
    expect(suggestName(k, { url: 'C:\\drops\\acme' })).toBe('acme');
    expect(suggestName(k, {})).toBe('Kind label');
    expect(suggestName(k, { url: '///' })).toBe('Kind label');
    expect(suggestName(null, { url: 'x' })).toBe('');
    expect(suggestName(SOURCE_KINDS[0]!, { path: '/srv/x/y' })).toBe('y');
  });

  it('finds, toggles and replaces actions', () => {
    const actions = blankDraft().actions;
    expect(findAction(actions, 'open-location')).toEqual({ kind: 'open-location' });
    expect(findAction(actions, 'download')).toBeNull();
    const on = toggleAction(actions, 'download', true);
    expect(findAction(on, 'download')).toEqual({ kind: 'download', folder: '', verify: true, variant: '', extension: '' });
    expect(findAction(toggleAction(on, 'run-command', true), 'run-command')).toEqual({ kind: 'run-command', program: '', args: [] });
    expect(toggleAction(on, 'open-location', true)).toHaveLength(2);
    expect(toggleAction(on, 'download', false).map((a) => a.kind)).toEqual(['open-location']);
    const replaced = replaceAction(on, { kind: 'download', folder: '/x', verify: false });
    expect(findAction(replaced, 'download')).toEqual({ kind: 'download', folder: '/x', verify: false });
    expect(replaced).toHaveLength(2);
  });

  it('parses arguments one per line', () => {
    expect(parseArgs(' a \r\n\n b c\n')).toEqual(['a', 'b c']);
    expect(parseArgs('')).toEqual([]);
  });
});

describe('product and extension helpers', () => {
  it('toggles extensions in a comma list, keeping custom ones', () => {
    expect(extensionList(' ISO, .exe;*.msi  tar.gz')).toEqual(['iso', 'exe', 'msi', 'tar.gz']);
    expect(toggleExtension('iso, exe', 'zip', true)).toBe('iso, exe, zip');
    expect(toggleExtension('iso, exe, zip', 'exe', false)).toBe('iso, zip');
    expect(toggleExtension('', 'iso', true)).toBe('iso');
    expect(toggleExtension('iso', 'iso', false)).toBe('');
    expect(EXTENSION_CHIPS).toContain('dmg');
  });

  it('narrows and widens the product list', () => {
    const all = ['PRO', 'QA', 'LITE'];
    expect(toggleTracked([], all, 'QA', false)).toEqual(['PRO', 'LITE']);
    expect(toggleTracked(['PRO', 'LITE'], all, 'QA', true)).toEqual([]);
    expect(toggleTracked(['PRO'], all, 'QA', true)).toEqual(['PRO', 'QA']);
    expect(toggleTracked(['PRO'], all, 'PRO', false)).toEqual([]);
    expect(toggleTracked([], [], 'X', false)).toEqual([]);
  });

  it('keeps only the product settings that differ from the defaults', () => {
    const t = blankDraft().tracking;
    const named = patchProduct(t, 'PRO', { name: 'Acme Installer Pro' });
    expect(named.products.settings['PRO']).toEqual({ name: 'Acme Installer Pro', muted: false, soakHours: null });
    const muted = patchProduct(named, 'PRO', { muted: true, soakHours: 6 });
    expect(muted.products.settings['PRO']).toEqual({ name: 'Acme Installer Pro', muted: true, soakHours: 6 });
    expect(patchProduct(patchProduct(named, 'PRO', { name: '  ' }), 'PRO', {}).products.settings).toEqual({});
  });

  it('adds the artifact choice to new actions', () => {
    expect(toggleAction([], 'open-location', true)).toEqual([{ kind: 'open-location', variant: '', extension: '' }]);
    expect(toggleAction([], 'download', true)).toEqual([{ kind: 'download', folder: '', verify: true, variant: '', extension: '' }]);
  });
});
