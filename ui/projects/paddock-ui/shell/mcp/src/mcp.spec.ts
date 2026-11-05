import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MAX_KEYS, MCP_EVENT } from '../generated/mcp';
import { MCP_COMMANDS, type McpCommand } from '../generated/mcp-commands';
import { McpBackend, TauriMcpBackend, type McpIpc } from './backend';
import { MemoryMcpBackend, type MemoryMcpOptions } from './memory';
import { McpStore } from './mcp.store';
import { provideMcpBackend, provideShellMcp } from './provide-mcp';
import { MCP_KEY_DIALOG_COMMANDS, MCP_SETTINGS_COMMANDS, defineMcpKeyDialog, defineMcpRevokeDialog, defineMcpSettingsPage, mcpContributions } from './mcp.contributions';
import { asMcpError, mcpMessage, parseAuditRows, parseIssuedKey, parseMcpEvent, parseMcpView, parseSettings } from './wire';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function setup(options: MemoryMcpOptions = {}) {
  const memory = new MemoryMcpBackend(options);
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellMcp({ backend: () => memory })] });
  const store = TestBed.inject(McpStore);
  return { memory, store };
}

describe('the AI access store', () => {
  it('loads the view and the log, off by default, with the tools split into read-only and actions', async () => {
    const { store } = setup();
    expect(store.isLoading()).toBe(true);
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.settings()?.enabled).toBe(false);
    expect(store.readOnlyTools().map((t) => t.name)).toEqual(['search_events', 'errors_around', 'incident_summary']);
    expect(store.actionTools().map((t) => t.name)).toEqual(['export_report']);
    expect(store.readOnlyTools().every((t) => !t.exposed)).toBe(true);
    expect(store.auditLoaded()).toBe(true);
    expect(store.audit()).toEqual([]);
    expect(store.atKeyLimit()).toBe(false);
    expect(store.maxKeys()).toBe(MAX_KEYS);
    expect(store.listener()).toEqual({ kind: 'off' });
  });

  it('turns it on, switches a tool off and allows an action, each saved at once', async () => {
    const { store, memory } = setup();
    await settle();
    expect(await store.setEnabled(true)).toBe(true);
    expect(store.settings()?.enabled).toBe(true);
    expect(store.readOnlyTools().every((t) => t.exposed)).toBe(true);
    expect(store.actionTools()[0]?.exposed).toBe(false);
    await store.setToolOn('errors_around', false);
    expect(store.settings()?.disabledTools).toEqual(['errors_around']);
    expect(store.readOnlyTools().find((t) => t.name === 'errors_around')?.exposed).toBe(false);
    await store.setToolOn('errors_around', true);
    expect(store.settings()?.disabledTools).toEqual([]);
    await store.setActionAllowed('export_report', true);
    expect(store.actionTools()[0]?.exposed).toBe(true);
    await store.setActionAllowed('export_report', false);
    expect(store.settings()?.allowedActions).toEqual([]);
    await store.setResources(false);
    expect((await memory.get()).settings.resources).toBe(false);
    expect(store.saving()).toBe(false);
  });

  it('keeps the engine’s words when a change is refused, and hides them on dismiss', async () => {
    const { store } = setup({ saveError: { code: 'storeUnavailable', message: 'The settings file is read-only.', retryable: true } });
    await settle();
    expect(await store.setEnabled(true)).toBe(false);
    expect(store.saveError()).toBe('The settings file is read-only.');
    expect(store.settings()?.enabled).toBe(false);
    store.dismissSaveError();
    expect(store.saveError()).toBeNull();
  });

  it('refuses a change before the first answer without calling the engine', async () => {
    const { store } = setup({ getFailures: 1 });
    await settle();
    expect(await store.setEnabled(true)).toBe(false);
    expect(store.saving()).toBe(false);
  });

  it('shows a failed first ask as an error and Retry loads it', async () => {
    const { store } = setup({ getFailures: 1 });
    await settle();
    expect(store.requestError()).toContain('could not be read');
    expect(store.isLoaded()).toBe(false);
    await store.load();
    expect(store.isLoaded()).toBe(true);
    expect(store.requestError()).toBeNull();
  });

  it('keeps the page on screen when a quiet refresh fails and picks up a key issued in another window', async () => {
    const { store, memory } = setup();
    await settle();
    await memory.issueKey('Pitwall');
    await store.refresh();
    expect(store.keys().map((k) => k.client)).toEqual(['Pitwall']);
    const view = store.settings();
    const failing = new MemoryMcpBackend({ getFailures: 1 });
    // A refresh that fails leaves what is shown as it is.
    vi.spyOn(memory, 'get').mockRejectedValueOnce(new Error('offline'));
    await store.refresh();
    expect(store.settings()).toBe(view);
    expect(failing).toBeTruthy();
  });

  it('revokes a key, and keeps the engine’s words when it cannot', async () => {
    const { store, memory } = setup();
    await settle();
    const { id } = await memory.issueKey('Pitwall');
    await store.refresh();
    expect(await store.revoke(id)).toBe(true);
    expect(store.keys()[0]?.revoked).toBe(true);
    expect(await store.revoke(999)).toBe(false);
    expect(store.saveError()).toContain('does not exist any more');
  });

  it('keeps a revoke refusal as the engine worded it', async () => {
    const { store } = setup({ revokeError: { code: 'storeUnavailable', message: 'The credential store is locked.', retryable: true } });
    await settle();
    expect(await store.revoke(1)).toBe(false);
    expect(store.saveError()).toBe('The credential store is locked.');
  });

  it('knows when the key limit is reached', async () => {
    const { store, memory } = setup();
    await settle();
    for (let i = 0; i < MAX_KEYS; i++) await memory.issueKey(`client ${i}`);
    await store.refresh();
    expect(store.atKeyLimit()).toBe(true);
    await expect(memory.issueKey('one too many')).rejects.toMatchObject({ code: 'invalid' });
  });

  it('reloads the log when the engine says a call was recorded, and clears it', async () => {
    const { store, memory } = setup();
    await settle();
    memory.recordCall({ client: 'Pitwall', tool: 'search_events', arguments: { query: '<omitted: 3 characters>' }, resultBytes: 120, durationMs: 7, outcome: 'ok' });
    await settle();
    expect(store.audit().map((r) => r.tool)).toEqual(['search_events']);
    memory.recordCall({ client: 'Pitwall', tool: 'errors_around', arguments: {}, resultBytes: 0, durationMs: 1, outcome: 'refused' });
    await settle();
    expect(store.audit().map((r) => r.tool)).toEqual(['errors_around', 'search_events']);
    expect(await store.clearAudit()).toBe(true);
    expect(store.audit()).toEqual([]);
  });

  it('shows a failed log read as its own error and Retry reads it', async () => {
    const { store } = setup({ auditFailures: 1 });
    await settle();
    expect(store.isLoaded()).toBe(true);
    expect(store.auditError()).toContain('could not be read');
    await store.loadAudit();
    expect(store.auditError()).toBeNull();
    expect(store.auditLoaded()).toBe(true);
  });

  it('keeps the engine’s words when the log cannot be cleared', async () => {
    const { store, memory } = setup();
    await settle();
    vi.spyOn(memory, 'clearAudit').mockRejectedValueOnce({ code: 'failed', message: 'The log is busy.', retryable: true });
    expect(await store.clearAudit()).toBe(false);
    expect(store.saveError()).toBe('The log is busy.');
  });

  it('stops listening when its window is destroyed, and survives a backend that cannot listen', async () => {
    const memory = new MemoryMcpBackend();
    let off = 0;
    vi.spyOn(memory, 'listen').mockImplementation(async () => () => void off++);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellMcp({ backend: () => memory })] });
    TestBed.inject(McpStore);
    await settle();
    TestBed.resetTestingModule();
    expect(off).toBe(1);
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => undefined);
    const bad = new MemoryMcpBackend();
    vi.spyOn(bad, 'listen').mockRejectedValue(new Error('no events'));
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellMcp({ backend: () => bad })] });
    TestBed.inject(McpStore);
    await settle();
    expect(warn).toHaveBeenCalled();
  });
});

describe('the memory engine', () => {
  it('validates names the way the engine does', async () => {
    const m = new MemoryMcpBackend();
    const base = await m.get();
    await expect(m.save({ ...base.settings, disabledTools: ['nope'] })).rejects.toMatchObject({ code: 'invalid' });
    await expect(m.save({ ...base.settings, allowedActions: ['search_events'] })).rejects.toMatchObject({ code: 'invalid' });
    await expect(m.issueKey('  ')).rejects.toMatchObject({ code: 'invalid' });
    await expect(m.issueKey('x'.repeat(65))).rejects.toMatchObject({ code: 'invalid' });
    await expect(new MemoryMcpBackend({ issueError: { code: 'storeUnavailable', message: 'locked', retryable: true } }).issueKey('a')).rejects.toMatchObject({ message: 'locked' });
  });

  it('hands a key out once and lists it only by client, never by key', async () => {
    const m = new MemoryMcpBackend({ calls: [{ client: 'a', tool: 't', arguments: null, resultBytes: 1, durationMs: 1, outcome: 'ok' }] });
    const issued = await m.issueKey(' Pitwall ');
    expect(issued.key).toMatch(/^pmk_[0-9a-f]{64}$/);
    expect(issued.client).toBe('Pitwall');
    expect(JSON.stringify(await m.get())).not.toContain(issued.key);
    expect(await m.audit()).toHaveLength(1);
  });
});

describe('the Tauri backend', () => {
  function ipc(answers: Record<string, unknown>) {
    const calls: { command: string; args?: Record<string, unknown> }[] = [];
    const handlers: ((p: unknown) => void)[] = [];
    const port: McpIpc = {
      invoke: async <T>(command: McpCommand, args?: Record<string, unknown>) => {
        calls.push({ command, args });
        const a = answers[command];
        if (a instanceof Error) throw a;
        return a as T;
      },
      listen: async (event, h) => {
        expect(event).toBe(MCP_EVENT);
        handlers.push(h);
        return () => undefined;
      },
    };
    return { backend: new TauriMcpBackend(port), calls, handlers };
  }
  const VIEW = {
    settings: { enabled: true, resources: true, disabledTools: [], allowedActions: [] },
    tools: [{ name: 'a', title: null, description: 'd', action: false, exposed: true }],
    keys: [{ id: 1, client: 'Pitwall', createdMs: 5, revoked: false }],
    maxKeys: 16,
    listener: { kind: 'unavailable' },
    keysProblem: null,
  };

  it('names the engine’s commands and parses what comes back', async () => {
    const { backend, calls } = ipc({
      mcp_get: VIEW,
      mcp_save: VIEW,
      mcp_revoke_key: VIEW,
      mcp_issue_key: { id: 2, client: 'x', key: 'pmk_abc' },
      mcp_audit: [{ atMs: 1, client: 'c', tool: 't', arguments: { q: 1 }, resultBytes: 2, durationMs: 3, outcome: 'ok' }],
      mcp_audit_clear: null,
    });
    expect((await backend.get()).keys[0]?.client).toBe('Pitwall');
    await backend.save(VIEW.settings);
    expect((await backend.issueKey('x')).key).toBe('pmk_abc');
    await backend.revokeKey(1);
    expect(await backend.audit()).toHaveLength(1);
    await backend.clearAudit();
    expect(calls.map((c) => c.command)).toEqual(['mcp_get', 'mcp_save', 'mcp_issue_key', 'mcp_revoke_key', 'mcp_audit', 'mcp_audit_clear']);
    expect(calls[2]?.args).toEqual({ client: 'x' });
    expect(calls[3]?.args).toEqual({ id: 1 });
    for (const c of calls) expect(MCP_COMMANDS).toHaveProperty(c.command);
  });

  it('refuses payloads that are not what the engine promises', async () => {
    const { backend } = ipc({ mcp_get: { nope: 1 }, mcp_issue_key: { id: 1 }, mcp_audit: 'x' });
    await expect(backend.get()).rejects.toThrow('no valid AI access settings');
    await expect(backend.issueKey('x')).rejects.toThrow('did not send a key');
    await expect(backend.audit()).rejects.toThrow('no valid activity log');
  });

  it('passes valid events on and drops the others', async () => {
    const { backend, handlers } = ipc({});
    const seen: unknown[] = [];
    await backend.listen((e) => seen.push(e));
    handlers[0]?.({ kind: 'audit', count: 3 });
    handlers[0]?.({ kind: 'other' });
    expect(seen).toEqual([{ kind: 'audit', count: 3 }]);
  });
});

describe('parsing at the edge', () => {
  it('rebuilds known fields only and rejects the rest', () => {
    expect(parseSettings({ enabled: true, resources: false, disabledTools: ['a'], allowedActions: [], extra: 1 })).toEqual({ enabled: true, resources: false, disabledTools: ['a'], allowedActions: [] });
    for (const bad of [null, [], { enabled: 'yes' }, { enabled: true, resources: true, disabledTools: [1], allowedActions: [] }]) expect(parseSettings(bad)).toBeNull();
    const tool = { name: 'a', title: 'A', description: 'd', action: true, exposed: false };
    const key = { id: 1, client: 'c', createdMs: 1, revoked: true };
    const ok = { settings: { enabled: false, resources: true, disabledTools: [], allowedActions: [] }, tools: [tool], keys: [key], maxKeys: 4, listener: { kind: 'off' }, keysProblem: null };
    expect(parseMcpView(ok)?.keysProblem).toBeNull();
    expect(parseMcpView({ settings: ok.settings, tools: ok.tools, keys: ok.keys, maxKeys: 4 })?.keysProblem).toBeNull();
    expect(parseMcpView({ ...ok, keysProblem: 'The key list could not be read.' })?.keysProblem).toBe('The key list could not be read.');
    for (const bad of [3, '', {}]) expect(parseMcpView({ ...ok, keysProblem: bad })?.keysProblem).toBeNull();
    expect(parseMcpView(ok)).toEqual(ok);
    for (const l of [{ kind: 'noKey' }, { kind: 'listening', port: 49152, discoveryFile: 'mcp-listener.json' }, { kind: 'failed', message: 'x' }]) expect(parseMcpView({ ...ok, listener: l })?.listener).toEqual(l);
    for (const bad of [undefined, 3, { kind: 'listening', port: -1, discoveryFile: 'f' }, { kind: 'failed' }, { kind: 'weird' }]) expect(parseMcpView({ ...ok, listener: bad })?.listener).toEqual({ kind: 'unavailable' });
    for (const bad of [null, { ...ok, tools: [{ ...tool, action: 1 }] }, { ...ok, tools: [{ ...tool, title: 3 }] }, { ...ok, keys: [{ ...key, id: -1 }] }, { ...ok, maxKeys: 'x' }, { ...ok, settings: 3 }, { ...ok, tools: 3 }]) expect(parseMcpView(bad)).toBeNull();
    expect(parseIssuedKey({ id: 1, client: 'c', key: '' })).toBeNull();
    expect(parseIssuedKey(null)).toBeNull();
    const row = { atMs: 1, client: 'c', tool: 't', arguments: { a: 1 }, resultBytes: 1, durationMs: 1, outcome: 'timed-out' };
    expect(parseAuditRows([row])).toEqual([row]);
    expect(parseAuditRows([{ ...row, outcome: 'weird' }])).toBeNull();
    expect(parseAuditRows([{ ...row, atMs: 'x' }])).toBeNull();
    expect(parseAuditRows({})).toBeNull();
    expect(parseAuditRows([{ ...row, arguments: undefined }])?.[0]?.arguments).toBeNull();
    expect(parseMcpEvent({ kind: 'audit', count: 2 })).toEqual({ kind: 'audit', count: 2 });
    expect(parseMcpEvent({ kind: 'audit', count: -1 })).toBeNull();
    expect(parseMcpEvent(null)).toBeNull();
  });

  it('turns anything a command rejected with into a message for the user', () => {
    expect(asMcpError({ code: 'invalid', message: 'm', retryable: true })).toEqual({ code: 'invalid', message: 'm', retryable: true });
    expect(asMcpError({ code: 'weird', message: 'm' })).toBeNull();
    expect(mcpMessage({ code: 'failed', message: 'engine says' }, 'x')).toBe('engine says');
    expect(mcpMessage(new Error('plain'), 'x')).toBe('plain');
    expect(mcpMessage('text', 'x')).toBe('text');
    expect(mcpMessage(7, 'fallback')).toBe('fallback');
  });
});

describe('the contributions', () => {
  it('declare the page, a credential form for the key and a decision for the revoke', () => {
    const page = defineMcpSettingsPage({ providers: async () => [], load: async () => class {} });
    expect(page.id).toBe('pd.mcp');
    const key = defineMcpKeyDialog({ providers: async () => [], load: async () => class {} });
    expect(key).toMatchObject({ id: 'pd.mcp.key', kind: 'form', modality: 'modal', commands: ['mcp_get', 'mcp_issue_key'] });
    const revoke = defineMcpRevokeDialog({ load: async () => class {} });
    expect(revoke).toMatchObject({ id: 'pd.mcp.revoke', kind: 'decision', instanced: true, commands: [] });
    const batch = mcpContributions(page, [key, revoke]);
    expect(batch.settingsPages).toHaveLength(1);
    expect(batch.dialogs).toHaveLength(2);
  });

  it('keep the key command out of the Settings window and in the key dialog only', () => {
    expect(MCP_SETTINGS_COMMANDS).not.toContain('mcp_issue_key');
    expect(MCP_KEY_DIALOG_COMMANDS).toEqual(['mcp_get', 'mcp_issue_key']);
    // It reads the key count on open, so the limit is a validation message before Create key, not a refusal after it.
    for (const c of [...MCP_SETTINGS_COMMANDS, ...MCP_KEY_DIALOG_COMMANDS]) expect(MCP_COMMANDS).toHaveProperty(c);
    expect(MCP_COMMANDS.mcp_issue_key.windows).toEqual(['dialog']);
  });

  it('give the key dialog its backend without the store', () => {
    const memory = new MemoryMcpBackend();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideMcpBackend({ backend: () => memory })] });
    expect(TestBed.inject(McpBackend)).toBe(memory);
    expect(() => TestBed.inject(McpStore)).toThrow();
  });
});
