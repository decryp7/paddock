import { SETTINGS_CHANGED_FIXTURE, SETTINGS_OVERVIEW_FIXTURE, SETTINGS_PROBLEM_FIXTURE, SETTINGS_REFUSED_FIXTURE } from '../generated/settings-fixtures';
import { SETTINGS_EVENT, type SettingField, type SettingsChanged } from '../generated/settings';
import { MemorySettingsBackend, memoryValidate, parseOverview, parseSetResult, parseSettingsChanged, TauriSettingsBackend, type SettingsIpc } from './settings-backend';

const fieldOf = (section: string, key: string): SettingField =>
  SETTINGS_OVERVIEW_FIXTURE.sections.find((s) => s.id === section)!.fields.find((f) => f.key === key)!;

describe('settings wire', () => {
  it('decodes what Rust wrote (fixtures from Rust round trips)', () => {
    const overview = parseOverview(SETTINGS_OVERVIEW_FIXTURE);
    expect(overview.sections.map((s) => s.id)).toEqual(['pd.appearance', 'sd.general', 'sd.storage']);
    const kinds = new Set(overview.sections.flatMap((s) => s.fields.map((f) => f.kind.type)));
    expect([...kinds].sort()).toEqual(['bool', 'duration', 'enum', 'float', 'int', 'path', 'size', 'stringList', 'text']);
    expect(parseSetResult(SETTINGS_REFUSED_FIXTURE)).toEqual(SETTINGS_REFUSED_FIXTURE);
    expect(SETTINGS_REFUSED_FIXTURE.errors.map((e) => e.key)).toEqual(['greeting', 'repeat']);
    expect(parseSettingsChanged(SETTINGS_CHANGED_FIXTURE)).toEqual(SETTINGS_CHANGED_FIXTURE);
    expect(parseOverview({ ...SETTINGS_OVERVIEW_FIXTURE, problem: SETTINGS_PROBLEM_FIXTURE }).problem?.kind).toBe('damaged');
  });

  it('drops what is not a settings payload', () => {
    expect(() => parseOverview(null)).toThrow(/not a list of settings/);
    expect(() => parseOverview({ sections: 'x' })).toThrow();
    const odd = parseOverview({ sections: [{ id: 1 }, { id: 'a', title: 'A', fields: [{ key: 'k' }], values: {} }, SETTINGS_OVERVIEW_FIXTURE.sections[0]], problem: 'x' });
    expect(odd.sections.map((s) => s.id)).toEqual(['pd.appearance']);
    expect(odd.problem).toBeNull();
    expect(odd.restartPending).toBe(false);
    expect(() => parseSetResult({ applied: true, errors: [], values: { a: { nested: 1 } } })).toThrow(/result of a change/);
    expect(() => parseSetResult({ applied: 'yes' })).toThrow();
    expect(parseSettingsChanged({ section: 'a', values: { n: Number.NaN } })).toBeNull();
    expect(parseSettingsChanged('x')).toBeNull();
    expect(parseSettingsChanged({ section: 'a', values: { n: 1, l: ['x'] } })).toEqual({ section: 'a', values: { n: 1, l: ['x'] }, restartPending: false });
  });
});

describe('TauriSettingsBackend', () => {
  function ipc(answers: Record<string, unknown>) {
    const calls: [string, unknown][] = [];
    let handler: ((p: unknown) => void) | null = null;
    const port: SettingsIpc = {
      invoke: <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return Promise.resolve(answers[command] as T);
      },
      listen: (event, h) => {
        expect(event).toBe(SETTINGS_EVENT);
        handler = h;
        return Promise.resolve(() => (handler = null));
      },
    };
    return { port, calls, emit: (p: unknown) => handler?.(p), listening: () => handler !== null };
  }

  it('calls the adapter’s commands with their arguments and parses the answers', async () => {
    const fake = ipc({ settings_overview: SETTINGS_OVERVIEW_FIXTURE, settings_set: SETTINGS_REFUSED_FIXTURE, settings_reset: SETTINGS_REFUSED_FIXTURE });
    const backend = new TauriSettingsBackend(fake.port);
    expect((await backend.overview()).sections).toHaveLength(3);
    await backend.set('sd.general', { greeting: 'Hi', repeat: null });
    await backend.reset('sd.general');
    await backend.reset('sd.general', 'repeat');
    expect(fake.calls).toEqual([
      ['settings_overview', undefined],
      ['settings_set', { section: 'sd.general', values: { greeting: 'Hi', repeat: null } }],
      ['settings_reset', { section: 'sd.general' }],
      ['settings_reset', { section: 'sd.general', key: 'repeat' }],
    ]);
  });

  it('passes changes from the event on and drops payloads that are not changes', async () => {
    const fake = ipc({});
    const seen: SettingsChanged[] = [];
    const off = await new TauriSettingsBackend(fake.port).listen((c) => seen.push(c));
    fake.emit(SETTINGS_CHANGED_FIXTURE);
    fake.emit({ nope: true });
    expect(seen).toEqual([SETTINGS_CHANGED_FIXTURE]);
    off();
    expect(fake.listening()).toBe(false);
  });
});

describe('MemorySettingsBackend', () => {
  it('lays changes over the values, refuses a set as a whole and announces each real change once', async () => {
    const backend = new MemorySettingsBackend();
    const seen: SettingsChanged[] = [];
    const off = await backend.listen((c) => seen.push(c));
    const refused = await backend.set('sd.general', { greeting: 'Hi there!', repeat: 2 });
    expect(refused.applied).toBe(false);
    expect(refused.errors).toEqual([{ key: 'greeting', message: 'Does not have the expected format ([A-Za-z ,]+).' }]);
    expect(backend.values('sd.general')?.['repeat']).toBe(1);
    const r = await backend.set('sd.general', { greeting: 'Hi', volume: 0.8, unknown: 1 });
    expect(r).toMatchObject({ applied: true, restartRequired: true });
    expect(r.values).toMatchObject({ greeting: 'Hi', volume: 0.8, repeat: 1 });
    await backend.set('sd.general', { greeting: 'Hi' });
    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ section: 'sd.general', restartPending: true });
    expect((await backend.overview()).restartPending).toBe(true);
    const reset = await backend.reset('sd.general', 'greeting');
    expect(reset.values['greeting']).toBe('Hello');
    expect((await backend.reset('sd.general')).values['volume']).toBe(0.5);
    off();
    await backend.set('sd.general', { repeat: 3 });
    expect(seen).toHaveLength(3);
  });

  it('answers unknown sections and fields with the engine’s errors', async () => {
    const backend = new MemorySettingsBackend(SETTINGS_OVERVIEW_FIXTURE, { problem: SETTINGS_PROBLEM_FIXTURE });
    expect((await backend.overview()).problem).toEqual(SETTINGS_PROBLEM_FIXTURE);
    await expect(backend.set('sd.none', {})).rejects.toMatchObject({ code: 'unknownSection' });
    await expect(backend.reset('sd.none')).rejects.toMatchObject({ code: 'unknownSection' });
    await expect(backend.reset('sd.general', 'nope')).rejects.toMatchObject({ code: 'unknownField' });
    expect(backend.values('sd.none')).toBeUndefined();
    const custom = new MemorySettingsBackend(SETTINGS_OVERVIEW_FIXTURE, { validate: () => 'never' });
    expect((await custom.set('sd.general', { repeat: 2 })).errors[0]?.message).toBe('never');
  });

  it('refuses what Rust would for every kind (a stand-in, Rust is the reference)', () => {
    expect(memoryValidate(fieldOf('sd.general', 'notify'), 'yes')).toContain('on or off');
    expect(memoryValidate(fieldOf('sd.general', 'notify'), false)).toBeNull();
    expect(memoryValidate(fieldOf('sd.general', 'style'), 'loud')).toBe('Must be one of: casual, formal.');
    expect(memoryValidate(fieldOf('sd.general', 'greeting'), 3)).toBe('Must be text.');
    expect(memoryValidate(fieldOf('sd.general', 'repeat'), 11)).toBe('Between 1–10 times.');
    expect(memoryValidate(fieldOf('sd.storage', 'folder'), 'data')).toContain('absolute');
    expect(memoryValidate(fieldOf('sd.storage', 'folder'), 3)).toBe('Must be a path.');
    expect(memoryValidate(fieldOf('sd.storage', 'folder'), 'C:\\data')).toBeNull();
    expect(memoryValidate(fieldOf('sd.storage', 'ignore'), 'x')).toContain('list');
    expect(memoryValidate(fieldOf('sd.storage', 'keep'), 120_000)).toBeNull();
  });
});
