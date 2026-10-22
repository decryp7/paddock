import type { AppPermissions, LiveUpdate, PlaygroundEvent, PlaygroundRequest, Settings } from './api.types';
import type { PitwallBackend } from './backend';
import { LIVE_EVENT, TauriBackend, type TauriBridge } from './tauri-backend';

interface Call {
  cmd: string;
  args?: Record<string, unknown>;
}

function fakeBridge(reply: (cmd: string) => unknown = () => null) {
  const calls: Call[] = [];
  const listeners: ((p: unknown) => void)[] = [];
  const unlistened: string[] = [];
  const channels: ((m: unknown) => void)[] = [];
  const bridge: TauriBridge = {
    invoke: <T>(cmd: string, args?: Record<string, unknown>) => {
      calls.push(args === undefined ? { cmd } : { cmd, args });
      const r = reply(cmd);
      return (r instanceof Promise ? r : Promise.resolve(r)) as Promise<T>;
    },
    listen: <T>(event: string, handler: (p: T) => void) => {
      listeners.push(handler as (p: unknown) => void);
      return Promise.resolve(() => void unlistened.push(event));
    },
    channel: <T>(onMessage: (m: T) => void) => {
      channels.push(onMessage as (m: unknown) => void);
      return { channel: channels.length };
    },
  };
  return { bridge, calls, listeners, unlistened, channels };
}

const PERMS: AppPermissions = {
  models: null,
  maxPriority: 'interactive',
  tools: false,
  extendedApis: false,
  limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null },
};
const SETTINGS = { general: { theme: 'dark' } } as unknown as Settings;

/**
 * The command table the Rust side implements (`app/src-tauri/src/commands.rs`). Each row: the call, the command
 * name and the exact arguments. Changing a row is a contract change: mirror it in Rust in the same commit.
 */
const TABLE: [string, (b: PitwallBackend) => Promise<unknown>, string, Record<string, unknown> | undefined][] = [
  ['overview', (b) => b.overview(), 'overview', undefined],
  ['history', (b) => b.history('24h'), 'history', { range: '24h' }],
  ['events', (b) => b.events(50), 'events', { limit: 50 }],
  ['setServing', (b) => b.setServing(true), 'set_serving', { paused: true }],
  ['releaseMemory', (b) => b.releaseMemory(), 'release_memory', undefined],
  ['applyPreset', (b) => b.applyPreset('lowMemory'), 'apply_preset', { preset: 'lowMemory' }],
  ['cancelJob', (b) => b.cancelJob('j1'), 'cancel_job', { id: 'j1' }],
  ['localModels', (b) => b.localModels(), 'local_models', undefined],
  ['searchModels', (b) => b.searchModels('qwen'), 'search_models', { query: 'qwen' }],
  ['repoFiles', (b) => b.repoFiles('a/b'), 'repo_files', { repo: 'a/b' }],
  ['downloadModel', (b) => b.downloadModel('a/b', 'f.gguf', null), 'download_model', { repo: 'a/b', file: 'f.gguf', projector: null }],
  ['downloads', (b) => b.downloads(), 'downloads', undefined],
  ['cancelDownload', (b) => b.cancelDownload('d1'), 'cancel_download', { id: 'd1' }],
  ['deleteModel', (b) => b.deleteModel('m1'), 'delete_model', { id: 'm1' }],
  ['loadModel', (b) => b.loadModel('m1'), 'load_model', { id: 'm1' }],
  ['unloadModel', (b) => b.unloadModel('m1'), 'unload_model', { id: 'm1' }],
  ['pinModel', (b) => b.pinModel('m1', true), 'pin_model', { id: 'm1', pinned: true }],
  ['setModelIdle', (b) => b.setModelIdle('m1', null), 'set_model_idle', { id: 'm1', idleSecs: null }],
  ['diskUsage', (b) => b.diskUsage(), 'disk_usage', undefined],
  ['engines', (b) => b.engines(), 'engines', undefined],
  ['packs', (b) => b.packs('code'), 'packs', { tag: 'code' }],
  ['packs without a tag', (b) => b.packs(), 'packs', { tag: null }],
  ['setModelEngine', (b) => b.setModelEngine('m1', 'vllm-pack'), 'set_model_engine', { id: 'm1', engine: 'vllm-pack' }],
  ['installBuild', (b) => b.installBuild('cuda-13'), 'install_build', { buildId: 'cuda-13' }],
  ['removeBuild', (b) => b.removeBuild('cpu', 'b1'), 'remove_build', { buildId: 'cpu', tag: 'b1' }],
  ['selectBuild', (b) => b.selectBuild('auto'), 'select_build', { choice: 'auto' }],
  ['rollbackBuild', (b) => b.rollbackBuild(), 'rollback_build', undefined],
  ['apps', (b) => b.apps(), 'apps', undefined],
  ['createApp', (b) => b.createApp('Tool'), 'create_app', { name: 'Tool' }],
  ['deleteApp', (b) => b.deleteApp('a1'), 'delete_app', { id: 'a1' }],
  ['createKey', (b) => b.createKey('a1'), 'create_key', { appId: 'a1' }],
  ['revokeKey', (b) => b.revokeKey('a1', 'k1'), 'revoke_key', { appId: 'a1', keyId: 'k1' }],
  ['setPermissions', (b) => b.setPermissions('a1', PERMS), 'set_permissions', { appId: 'a1', permissions: PERMS }],
  ['approvePairing', (b) => b.approvePairing('p1', PERMS), 'approve_pairing', { id: 'p1', permissions: PERMS }],
  ['denyPairing', (b) => b.denyPairing('p1'), 'deny_pairing', { id: 'p1' }],
  ['pairings', (b) => b.pairings(), 'pairings', undefined],
  ['requests', (b) => b.requests(500), 'requests', { limit: 500 }],
  ['requestContent', (b) => b.requestContent('r1'), 'request_content', { id: 'r1' }],
  ['benchmarks', (b) => b.benchmarks(), 'benchmarks', undefined],
  ['runBenchmark', (b) => b.runBenchmark('m1'), 'run_benchmark', { modelId: 'm1' }],
  ['settings', (b) => b.settings(), 'settings', undefined],
  ['saveSettings', (b) => b.saveSettings(SETTINGS), 'save_settings', { settings: SETTINGS }],
  ['appUpdateStatus', (b) => b.appUpdateStatus(), 'app_update_status', undefined],
];

describe('TauriBackend', () => {
  it.each(TABLE)('%s invokes its command with the documented arguments', async (_name, call, cmd, args) => {
    const { bridge, calls } = fakeBridge();
    await call(new TauriBackend(bridge));
    expect(calls).toEqual([args === undefined ? { cmd } : { cmd, args }]);
  });

  it('covers every request/response method of the backend interface', () => {
    const methods = Object.getOwnPropertyNames(TauriBackend.prototype).filter((m) => m !== 'constructor' && m !== 'run');
    const pinned = new Set([...TABLE.map((r) => r[0]), 'subscribe', 'chat']);
    expect(methods.filter((m) => !pinned.has(m))).toEqual([]);
  });

  it('turns rejections into typed errors', async () => {
    const { bridge } = fakeBridge(() => Promise.reject({ code: 'offline', message: 'Offline mode is on.' }));
    await expect(new TauriBackend(bridge).searchModels('x')).rejects.toEqual({ code: 'offline', message: 'Offline mode is on.' });
    const odd = fakeBridge(() => Promise.reject(new Error('boom')));
    await expect(new TauriBackend(odd.bridge).overview()).rejects.toEqual({ code: 'internal', message: 'boom' });
  });

  it('subscribes to the live event and stops on unsubscribe', async () => {
    const { bridge, listeners, unlistened } = fakeBridge();
    const got: LiveUpdate[] = [];
    const stop = new TauriBackend(bridge).subscribe((u) => got.push(u));
    await Promise.resolve();
    const update: LiveUpdate = { kind: 'pairing', pairings: [] };
    listeners[0]!(update);
    expect(got).toEqual([update]);
    stop();
    listeners[0]!(update);
    expect(got.length).toBe(1);
    expect(unlistened).toEqual([LIVE_EVENT]);
  });

  it('unlistens at once when unsubscribed before listening resolved', async () => {
    const { bridge, unlistened } = fakeBridge();
    const stop = new TauriBackend(bridge).subscribe(() => undefined);
    stop();
    await Promise.resolve();
    await Promise.resolve();
    expect(unlistened).toEqual([LIVE_EVENT]);
  });

  it('survives a failing listen', async () => {
    const bridge: TauriBridge = { ...fakeBridge().bridge, listen: () => Promise.reject(new Error('no ipc')) };
    const stop = new TauriBackend(bridge).subscribe(() => undefined);
    await Promise.resolve();
    expect(() => stop()).not.toThrow();
  });

  const REQ: PlaygroundRequest = { modelId: 'm1', messages: [{ role: 'user', content: 'hi' }], temperature: 0.7, maxTokens: 64 };

  it('streams chat over a channel and cancels by stream id', async () => {
    const { bridge, calls, channels } = fakeBridge((cmd) => (cmd === 'chat' ? 'stream-1' : null));
    const events: PlaygroundEvent[] = [];
    const stop = new TauriBackend(bridge).chat(REQ, (e) => events.push(e));
    await Promise.resolve();
    await Promise.resolve();
    expect(calls[0]).toEqual({ cmd: 'chat', args: { request: REQ, onEvent: { channel: 1 } } });
    channels[0]!({ kind: 'delta', text: 'Hel' });
    expect(events).toEqual([{ kind: 'delta', text: 'Hel' }]);
    stop();
    stop();
    channels[0]!({ kind: 'delta', text: 'lo' });
    expect(events.length).toBe(1);
    expect(calls.slice(1)).toEqual([{ cmd: 'chat_cancel', args: { id: 'stream-1' } }]);
  });

  it('cancels a chat stopped before its id arrived', async () => {
    const { bridge, calls } = fakeBridge((cmd) => (cmd === 'chat' ? 'stream-2' : null));
    const stop = new TauriBackend(bridge).chat(REQ, () => undefined);
    stop();
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(calls.map((c) => c.cmd)).toEqual(['chat', 'chat_cancel']);
  });

  it('reports a chat that failed to start as an error event', async () => {
    const { bridge } = fakeBridge((cmd) => (cmd === 'chat' ? Promise.reject({ code: 'notFound', message: 'No such model' }) : null));
    const events: PlaygroundEvent[] = [];
    new TauriBackend(bridge).chat(REQ, (e) => events.push(e));
    for (let i = 0; i < 4; i++) await Promise.resolve();
    expect(events).toEqual([{ kind: 'error', error: { code: 'notFound', message: 'No such model' } }]);
  });
});
