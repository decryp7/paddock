import type { ModelPreset, PresetDraft } from '../../core/api.types';
import { int } from '../../shared/format';

/** The editor's fields as the user typed them. Numbers stay text until they are parsed, so a half-typed value is not lost. */
export interface PresetForm {
  alias: string;
  target: string;
  context: string;
  temperature: string;
  topP: string;
  maxTokens: string;
  systemPrompt: string;
  cacheResponses: boolean;
  keepWarm: boolean;
}

/** The text fields; the two flags are set directly. */
export type PresetField = Exclude<keyof PresetForm, 'cacheResponses' | 'keepWarm'>;

export const EMPTY_FORM: PresetForm = { alias: '', target: '', context: '', temperature: '', topP: '', maxTokens: '', systemPrompt: '', cacheResponses: false, keepWarm: false };

/** A preset as form text. */
export function formOf(p: ModelPreset): PresetForm {
  const text = (n: number | null) => (n === null ? '' : String(n));
  return {
    alias: p.alias,
    target: p.target,
    context: text(p.context),
    temperature: text(p.temperature),
    topP: text(p.topP),
    maxTokens: text(p.maxTokens),
    systemPrompt: p.systemPrompt ?? '',
    cacheResponses: p.cacheResponses,
    keepWarm: p.keepWarm,
  };
}

/** A number field's text: empty is none, a plain number is the value, anything else is invalid. Ranges are the engine's to judge. */
export function parseNumber(text: string, whole: boolean): number | null | 'invalid' {
  const t = text.trim();
  if (t === '') return null;
  if (!(whole ? /^\d+$/ : /^\d*\.?\d+$/).test(t)) return 'invalid';
  const n = Number(t);
  return Number.isFinite(n) ? n : 'invalid';
}

export interface ParsedForm {
  draft: PresetDraft;
  /** The number fields that are not numbers. */
  invalid: ReadonlySet<PresetField>;
}

/** The form as a draft for the engine, with the fields that cannot be sent. */
export function parseForm(f: PresetForm): ParsedForm {
  const invalid = new Set<PresetField>();
  const num = (field: PresetField, whole: boolean): number | null => {
    const n = parseNumber(f[field], whole);
    if (n === 'invalid') {
      invalid.add(field);
      return null;
    }
    return n;
  };
  const draft: PresetDraft = {
    alias: f.alias.trim(),
    target: f.target.trim(),
    context: num('context', true),
    temperature: num('temperature', false),
    topP: num('topP', false),
    maxTokens: num('maxTokens', true),
    systemPrompt: f.systemPrompt.trim() === '' ? null : f.systemPrompt,
    cacheResponses: f.cacheResponses,
    keepWarm: f.keepWarm,
  };
  return { draft, invalid };
}

/** `0.3 temperature · 800 max tokens` style summary of a preset's defaults, for the list. */
export function defaultsSummary(p: ModelPreset): string[] {
  const out: string[] = [];
  if (p.context !== null) out.push(`context ${int(p.context)}`);
  if (p.temperature !== null) out.push(`temperature ${p.temperature}`);
  if (p.topP !== null) out.push(`top-p ${p.topP}`);
  if (p.maxTokens !== null) out.push(`max ${int(p.maxTokens)} tokens`);
  return out;
}
