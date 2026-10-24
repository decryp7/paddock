import type { SettingsCommand } from '../generated/settings-commands';
import { SETTINGS_OVERVIEW_FIXTURE } from '../generated/settings-fixtures';
import {
  SETTINGS_EVENT,
  type FieldError,
  type LoadProblem,
  type SectionView,
  type SetResult,
  type SettingField,
  type SettingsChanged,
  type SettingsOverview,
  type SettingValue,
} from '../generated/settings';
import { precheck } from './settings-model';

/** Stops listening. */
export type Unlisten = () => void;

/** What a change sends: a key's new value, or `null` for its default. Keys left out keep their value. */
export type SettingsChanges = Readonly<Record<string, SettingValue | null>>;

/**
 * The settings port (ADR 0012 §4): the commands of `paddock_settings::tauri` in the desktop app, an in-memory engine in
 * the browser build and in tests. Components and stores never call `invoke`. Validation, defaults and persistence are
 * Rust's; this only carries values. Nothing here has a timeout (rule 12).
 */
export abstract class SettingsBackend {
  /** Every section with its values, the restart state and the start-up problem (`settings_overview`). */
  abstract overview(): Promise<SettingsOverview>;
  /** Lays `changes` over the section's values; refused values come back as `errors` with `applied: false` (`settings_set`). */
  abstract set(section: string, changes: SettingsChanges): Promise<SetResult>;
  /** Puts a section, or only its field `key`, back to the default (`settings_reset`). */
  abstract reset(section: string, key?: string): Promise<SetResult>;
  /** Calls `handler` with every change that took effect, from any window (`paddock://settings`). */
  abstract listen(handler: (change: SettingsChanged) => void): Promise<Unlisten>;
}

/**
 * The app's IPC as the settings backend needs it, so this package does not depend on `@tauri-apps/api`: Tauri's `invoke`
 * and an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface SettingsIpc {
  invoke<T>(command: SettingsCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);
const isValue = (v: unknown): v is SettingValue =>
  typeof v === 'boolean' || (typeof v === 'number' && Number.isFinite(v)) || typeof v === 'string' || (Array.isArray(v) && v.every((x) => typeof x === 'string'));
const isValues = (v: unknown): v is Record<string, SettingValue> => isObj(v) && Object.values(v).every(isValue);

function isField(v: unknown): v is SettingField {
  return isObj(v) && typeof v['key'] === 'string' && typeof v['label'] === 'string' && isObj(v['kind']) && typeof v['kind']['type'] === 'string' && isValue(v['default']);
}

function isSection(v: unknown): v is SectionView {
  return isObj(v) && typeof v['id'] === 'string' && typeof v['title'] === 'string' && Array.isArray(v['fields']) && v['fields'].every(isField) && isValues(v['values']);
}

function isErrors(v: unknown): v is FieldError[] {
  return Array.isArray(v) && v.every((e) => isObj(e) && typeof e['key'] === 'string' && typeof e['message'] === 'string');
}

/** Parses a `settings_overview` answer (the webview never trusts a payload's shape); a section that is not one is dropped. */
export function parseOverview(v: unknown): SettingsOverview {
  if (!isObj(v) || !Array.isArray(v['sections'])) throw new Error('The settings service answered with something that is not a list of settings.');
  const p = v['problem'];
  const problem = isObj(p) && typeof p['message'] === 'string' ? (p as unknown as LoadProblem) : null;
  return { sections: v['sections'].filter(isSection), restartPending: v['restartPending'] === true, problem };
}

/** Parses a `settings_set` / `settings_reset` answer. */
export function parseSetResult(v: unknown): SetResult {
  if (!isObj(v) || typeof v['applied'] !== 'boolean' || !isErrors(v['errors']) || !isValues(v['values'])) {
    throw new Error('The settings service answered with something that is not the result of a change.');
  }
  return { applied: v['applied'], restartRequired: v['restartRequired'] === true, errors: v['errors'], values: v['values'] };
}

/** Parses one `paddock://settings` payload, or `null` when it is not a change. */
export function parseSettingsChanged(v: unknown): SettingsChanged | null {
  if (!isObj(v) || typeof v['section'] !== 'string' || !isValues(v['values'])) return null;
  return { section: v['section'], values: v['values'], restartPending: v['restartPending'] === true };
}

/** The settings backend over Tauri. Payloads are parsed at this edge. */
export class TauriSettingsBackend extends SettingsBackend {
  constructor(private readonly ipc: SettingsIpc) {
    super();
  }

  async overview(): Promise<SettingsOverview> {
    return parseOverview(await this.ipc.invoke<unknown>('settings_overview'));
  }

  async set(section: string, changes: SettingsChanges): Promise<SetResult> {
    return parseSetResult(await this.ipc.invoke<unknown>('settings_set', { section, values: changes }));
  }

  async reset(section: string, key?: string): Promise<SetResult> {
    return parseSetResult(await this.ipc.invoke<unknown>('settings_reset', key === undefined ? { section } : { section, key }));
  }

  listen(handler: (change: SettingsChanged) => void): Promise<Unlisten> {
    return this.ipc.listen(SETTINGS_EVENT, (payload) => {
      const change = parseSettingsChanged(payload);
      if (change) handler(change);
    });
  }
}

/**
 * What the in-memory engine refuses: the pre-check's types and ranges, enum options, a text's pattern (as a JavaScript
 * expression) and relative paths. A stand-in for `paddock_settings::validate_field`, which is the reference.
 */
export function memoryValidate(field: SettingField, value: SettingValue): string | null {
  const k = field.kind;
  const early = precheck(field, value);
  if (early) return early;
  switch (k.type) {
    case 'bool':
      return typeof value === 'boolean' ? null : 'Must be on or off (true or false).';
    case 'enum':
      return k.options.some((o) => o.value === value) ? null : `Must be one of: ${k.options.map((o) => o.value).join(', ')}.`;
    case 'text':
      if (typeof value !== 'string') return 'Must be text.';
      return k.pattern && !new RegExp(`^(?:${k.pattern})$`, 'u').test(value) ? `Does not have the expected format (${k.pattern}).` : null;
    case 'path':
      if (typeof value !== 'string') return 'Must be a path.';
      return value === '' || /^(\/|[A-Za-z]:[\\/]|\\\\)/.test(value) ? null : 'Must be an absolute path (or empty).';
    case 'stringList':
      return Array.isArray(value) ? null : 'Must be a list of text.';
    default:
      return null;
  }
}

/** Options of the in-memory engine. */
export interface MemorySettingsOptions {
  /** The start-up problem to report (a damaged settings file, for the demo and the e2e). */
  readonly problem?: LoadProblem | null;
  /** Decides what is refused; defaults to `memoryValidate`. */
  readonly validate?: (field: SettingField, value: SettingValue) => string | null;
}

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;

/**
 * A settings engine held in memory, for the browser build, demos and tests. It plays `paddock_settings::Settings`:
 * changes are laid over the current values, refused as a whole when any field is refused, and every change that took
 * effect reaches every listener once (the same values again announce nothing).
 */
export class MemorySettingsBackend extends SettingsBackend {
  private sections: SectionView[];
  private restartPending: boolean;
  private readonly problem: LoadProblem | null;
  private readonly check: (field: SettingField, value: SettingValue) => string | null;
  private readonly listeners = new Set<(change: SettingsChanged) => void>();

  constructor(overview: SettingsOverview = SETTINGS_OVERVIEW_FIXTURE, options: MemorySettingsOptions = {}) {
    super();
    this.sections = clone(overview.sections);
    this.restartPending = overview.restartPending;
    this.problem = options.problem ?? overview.problem;
    this.check = options.validate ?? memoryValidate;
  }

  /** The current values of `section` (tests). */
  values(section: string): Record<string, SettingValue> | undefined {
    const s = this.sections.find((x) => x.id === section);
    return s ? clone(s.values) : undefined;
  }

  overview(): Promise<SettingsOverview> {
    return Promise.resolve(clone({ sections: this.sections, restartPending: this.restartPending, problem: this.problem }));
  }

  set(section: string, changes: SettingsChanges): Promise<SetResult> {
    const s = this.sections.find((x) => x.id === section);
    if (!s) return Promise.reject({ code: 'unknownSection', message: `There is no settings section "${section}".`, retryable: false });
    const next: Record<string, SettingValue> = { ...s.values };
    const errors: FieldError[] = [];
    for (const field of s.fields) {
      if (!(field.key in changes)) continue;
      const v = changes[field.key];
      if (v === null || v === undefined) next[field.key] = field.default;
      else {
        const problem = this.check(field, v);
        if (problem) errors.push({ key: field.key, message: problem });
        else next[field.key] = v;
      }
    }
    if (errors.length) return Promise.resolve({ applied: false, restartRequired: false, errors, values: clone(s.values) });
    const restartRequired = s.fields.some((f) => f.restartRequired && JSON.stringify(s.values[f.key]) !== JSON.stringify(next[f.key]));
    const changed = JSON.stringify(next) !== JSON.stringify(s.values);
    if (changed) {
      this.sections = this.sections.map((x) => (x.id === section ? { ...x, values: next } : x));
      this.restartPending ||= restartRequired;
      const event: SettingsChanged = { section, values: clone(next), restartPending: this.restartPending };
      for (const l of [...this.listeners]) l(event);
    }
    return Promise.resolve({ applied: true, restartRequired, errors: [], values: clone(next) });
  }

  reset(section: string, key?: string): Promise<SetResult> {
    const s = this.sections.find((x) => x.id === section);
    if (!s) return Promise.reject({ code: 'unknownSection', message: `There is no settings section "${section}".`, retryable: false });
    if (key !== undefined && !s.fields.some((f) => f.key === key)) {
      return Promise.reject({ code: 'unknownField', message: `The settings section "${section}" has no setting "${key}".`, retryable: false });
    }
    const keys = key === undefined ? s.fields.map((f) => f.key) : [key];
    return this.set(section, Object.fromEntries(keys.map((k) => [k, null])));
  }

  listen(handler: (change: SettingsChanged) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
