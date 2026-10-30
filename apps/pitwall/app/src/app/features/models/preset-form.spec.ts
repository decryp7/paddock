import type { ModelPreset } from '../../core/api.types';
import { defaultsSummary, EMPTY_FORM, formOf, parseForm, parseNumber } from './preset-form';

const PRESET: ModelPreset = { alias: 'debrief-explain', target: 'auto:chat,fast', context: 8192, temperature: 0.3, topP: 0.9, maxTokens: 800, systemPrompt: 'Be brief.', cacheResponses: false, keepWarm: false };

describe('parseNumber', () => {
  it('reads plain numbers, empty as none, and rejects the rest', () => {
    expect(parseNumber('', true)).toBeNull();
    expect(parseNumber('  ', false)).toBeNull();
    expect(parseNumber('4096', true)).toBe(4096);
    expect(parseNumber('0.3', false)).toBe(0.3);
    expect(parseNumber('.5', false)).toBe(0.5);
    expect(parseNumber('2', false)).toBe(2);
    for (const bad of ['1.5', 'abc', '-1', '1e3', '0.3.1', '1,5']) expect(parseNumber(bad, true)).toBe('invalid');
    for (const bad of ['abc', '-0.3', '1e3', '0.3.1', '0.']) expect(parseNumber(bad, false)).toBe('invalid');
  });
});

describe('preset form', () => {
  it('round-trips a preset through the form text', () => {
    const f = formOf(PRESET);
    expect(f).toEqual({ alias: 'debrief-explain', target: 'auto:chat,fast', context: '8192', temperature: '0.3', topP: '0.9', maxTokens: '800', systemPrompt: 'Be brief.', cacheResponses: false, keepWarm: false });
    const { draft, invalid } = parseForm(f);
    expect(draft).toEqual(PRESET);
    expect(invalid.size).toBe(0);
  });

  it('an empty form is an empty draft; blanks are none', () => {
    const { draft, invalid } = parseForm({ ...EMPTY_FORM, alias: ' x ', target: ' m ', systemPrompt: '  \n' });
    expect(draft).toEqual({ alias: 'x', target: 'm', context: null, temperature: null, topP: null, maxTokens: null, systemPrompt: null, cacheResponses: false, keepWarm: false });
    expect(invalid.size).toBe(0);
    expect(formOf({ ...PRESET, context: null, temperature: null, topP: null, maxTokens: null, systemPrompt: null })).toEqual({ ...EMPTY_FORM, alias: PRESET.alias, target: PRESET.target });
  });

  it('names the number fields that are not numbers and sends none for them', () => {
    const { draft, invalid } = parseForm({ ...EMPTY_FORM, alias: 'a', target: 'm', context: 'big', temperature: '0,3', topP: '0.5', maxTokens: '12.5' });
    expect([...invalid].sort()).toEqual(['context', 'maxTokens', 'temperature']);
    expect(draft.topP).toBe(0.5);
    expect(draft.context).toBeNull();
  });

  it('summarises the defaults for the list', () => {
    expect(defaultsSummary(PRESET)).toEqual(['context 8,192', 'temperature 0.3', 'top-p 0.9', 'max 800 tokens']);
    expect(defaultsSummary({ ...PRESET, context: null, temperature: null, topP: null, maxTokens: null })).toEqual([]);
  });
});
