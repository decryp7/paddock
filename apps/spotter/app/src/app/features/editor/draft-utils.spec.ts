import type { FieldSpec, SourceKind } from '../../core/api.types';
import { SOURCE_KINDS } from '../../core/mock/mock-data';
import { blankDraft } from '../../core/draft';
import { defaultValues, draftFor, findAction, missingFields, parseArgs, replaceAction, suggestName, toggleAction } from './draft-utils';

const field = (key: string, over: Partial<FieldSpec> = {}): FieldSpec => ({
  key, label: key, help: '', kind: 'text', required: true, default: '', placeholder: '', min: null, max: null, options: [], ...over,
});
const kind = (fields: FieldSpec[]): SourceKind => ({ id: 'k', label: 'Kind label', description: '', group: 'files', fields } as SourceKind);

describe('draft utils', () => {
  it('collects only non-empty field defaults', () => {
    expect(defaultValues(kind([field('a', { default: 'x' }), field('b')]))).toEqual({ a: 'x' });
  });

  it('builds a fresh draft for a kind with the given interval, dropping the id', () => {
    const base = blankDraft({ id: 'old', name: 'Keep' });
    const d = draftFor(base, kind([field('a', { default: '1' })]), 900);
    expect(d).toMatchObject({ id: null, name: 'Keep', intervalSecs: 900, source: { kind: 'k', values: { a: '1' } } });
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
    expect(findAction(on, 'download')).toEqual({ kind: 'download', folder: '', verify: true });
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
