import { mockOptionsFrom } from '../app.config';
import type { LiveUpdate, PlaygroundEvent } from './api.types';
import { isTauri } from './backend';
import { toCommandError } from './command-error';
import { MockBackend } from './mock-backend';
import { demoFit, demoHistory, demoRepo, demoRequests, demoSearch, presetMemory, prng, GiB } from './mock-data';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();

describe('MockBackend', () => {
  let t = NOW;
  const clock = () => t;
  beforeEach(() => (t = NOW));

  it('detects the browser build and types errors', () => {
    expect(isTauri()).toBe(false);
    expect(toCommandError({ code: 'busy', message: 'later' })).toEqual({ code: 'busy', message: 'later' });
    expect(toCommandError({ code: 'weird', message: 'x' })).toEqual({ code: 'internal', message: 'Something went wrong.' });
    expect(toCommandError(new Error('boom'))).toEqual({ code: 'internal', message: 'boom' });
    expect(toCommandError('plain')).toEqual({ code: 'internal', message: 'plain' });
    expect(toCommandError(null)).toEqual({ code: 'internal', message: 'Something went wrong.' });
  });

  it('reads its options from the URL', () => {
    expect(mockOptionsFrom('')).toMatchObject({ latencyMs: 120, autoTick: true, empty: false, failing: [] });
    expect(mockOptionsFrom('?mock-fail=overview,history&mock-empty&mock-static&mock-fast')).toMatchObject({ latencyMs: 0, autoTick: false, empty: true, failing: ['overview', 'history'] });
  });

  it('returns copies of its demo data', async () => {
    const mock = new MockBackend({ now: clock });
    const a = await mock.localModels();
    a[0]!.name = 'changed';
    expect((await mock.localModels())[0]!.name).not.toBe('changed');
  });

  it('serves every read with plausible data', async () => {
    const mock = new MockBackend({ now: clock, latencyMs: 1 });
    const o = await mock.overview();
    expect(o.memory.ramTotal).toBe(32 * GiB);
    expect(o.engines[0]!.activeBuild).toBe('cuda-13 · b11669');
    expect(o.engines[1]!.activeBuild).toBeNull();
    for (const r of ['1h', '24h', '7d', '30d'] as const) {
      const h = await mock.history(r);
      expect(h.points.length).toBeGreaterThan(100);
      expect(h.requestsByApp[0]!.values.length).toBe(h.points.length);
      expect(h.events.some((e) => e.kind === 'modelUnloaded')).toBe(true);
    }
    expect(await mock.history('1h')).toEqual(await mock.history('1h'));
    expect((await mock.events(3)).length).toBe(3);
    expect((await mock.searchModels('qwen')).every((m) => m.repo.toLowerCase().includes('qwen'))).toBe(true);
    expect((await mock.repoFiles('unsloth/gemma-3-12b-it-GGUF')).projectors.length).toBe(1);
    expect((await mock.engines()).length).toBe(5);
    expect((await mock.apps()).length).toBe(5);
    expect((await mock.pairings()).length).toBe(1);
    expect((await mock.requests(10)).length).toBe(10);
    expect((await mock.benchmarks()).length).toBeGreaterThan(0);
    expect((await mock.plugins()).length).toBe(3);
    expect((await mock.mcpServers()).length).toBe(2);
    expect((await mock.settings()).memory.preset).toBe('balanced');
    expect((await mock.appUpdateStatus()).available).toBe(false);
    expect((await mock.diskUsage()).unused).toContain('qwen2.5-coder-1.5b-q8_0');
  });

  it('starts empty on request and fails the chosen methods', async () => {
    const empty = new MockBackend({ empty: true });
    expect((await empty.overview()).models).toEqual([]);
    expect((await empty.history('7d')).points).toEqual([]);
    const failing = new MockBackend({ failing: ['apps'] });
    await expect(failing.apps()).rejects.toMatchObject({ code: 'serverUnavailable' });
  });

  it('pushes live updates while subscribed, on ticks', () => {
    const mock = new MockBackend({ now: clock });
    const got: LiveUpdate[] = [];
    const stop = mock.subscribe((u) => got.push(u));
    mock.tick();
    mock.tick();
    expect(got.map((u) => u.kind)).toEqual(expect.arrayContaining(['memory', 'throughput', 'queue', 'download']));
    stop();
    const n = got.length;
    mock.tick();
    expect(got.length).toBe(n);
  });

  it('ticks on its own with autoTick and stops when nobody listens', () => {
    vi.useFakeTimers();
    try {
      const mock = new MockBackend({ autoTick: true });
      const got: LiveUpdate[] = [];
      const stop = mock.subscribe((u) => got.push(u));
      vi.advanceTimersByTime(2100);
      expect(got.filter((u) => u.kind === 'memory').length).toBe(2);
      stop();
      vi.advanceTimersByTime(3000);
      expect(got.filter((u) => u.kind === 'memory').length).toBe(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it('releases a model when its idle time is up', async () => {
    const mock = new MockBackend({ now: clock });
    const got: LiveUpdate[] = [];
    mock.subscribe((u) => got.push(u));
    t = NOW + 11 * 60_000;
    mock.tick();
    const models = got.find((u) => u.kind === 'models');
    expect(models && models.kind === 'models' && models.models.some((m) => m.modelId === 'qwen3-8b-instruct-q4_k_m')).toBe(false);
    expect(got.some((u) => u.kind === 'event' && u.event.reason === 'idle')).toBe(true);
  });

  it('downloads models and engine builds to completion, and cancels', async () => {
    const mock = new MockBackend({ now: clock });
    const d = await mock.downloadModel('Qwen/Qwen3-8B-Instruct-GGUF', 'Qwen3-8B-Instruct-Q2_K.gguf', null);
    expect(d.state).toBe('queued');
    const e = await mock.installBuild('vulkan');
    for (let i = 0; i < 400; i++) mock.tick();
    const list = await mock.downloads();
    expect(list.find((x) => x.id === d.id)?.state).toBe('done');
    expect(list.find((x) => x.id === e.id)?.state).toBe('done');
    expect((await mock.localModels()).some((m) => m.file === 'Qwen3-8B-Instruct-Q2_K.gguf')).toBe(true);
    expect((await mock.engines())[0]!.builds.find((b) => b.id === 'vulkan')?.tag).toBe('b11702');
    await mock.cancelDownload('dl-0');
    expect((await mock.downloads()).find((x) => x.id === 'dl-0')?.state).toBe('cancelled');
    await expect(mock.cancelDownload('nope')).rejects.toMatchObject({ code: 'notFound' });
    await expect(mock.installBuild('nope')).rejects.toMatchObject({ code: 'notFound' });
  });

  it('refuses network work in offline mode', async () => {
    const mock = new MockBackend({ now: clock });
    const s = await mock.settings();
    await mock.saveSettings({ ...s, network: { ...s.network, offline: true, lanEnabled: true } });
    expect((await mock.overview()).server.lan.url).toContain('https://');
    await expect(mock.searchModels('x')).rejects.toMatchObject({ code: 'offline' });
    await expect(mock.installBuild('cpu')).rejects.toMatchObject({ code: 'offline' });
    await mock.saveSettings({ ...s, network: { ...s.network, offline: false, lanEnabled: false } });
    expect((await mock.overview()).server.lan.enabled).toBe(false);
    await expect(mock.saveSettings({ ...s, server: { ...s.server, port: 80 } })).rejects.toMatchObject({ code: 'badRequest' });
  });

  it('manages the model library', async () => {
    const mock = new MockBackend({ now: clock });
    await expect(mock.deleteModel('qwen3-8b-instruct-q4_k_m')).rejects.toMatchObject({ code: 'conflict' });
    await mock.deleteModel('qwen2.5-coder-1.5b-q8_0');
    expect((await mock.localModels()).length).toBe(5);
    await mock.loadModel('gemma-3-12b-it-q4_k_m');
    await mock.loadModel('gemma-3-12b-it-q4_k_m');
    await expect(mock.loadModel('nope')).rejects.toMatchObject({ code: 'notFound' });
    await mock.pinModel('gemma-3-12b-it-q4_k_m', true);
    await mock.pinModel('bge-reranker-v2-m3-q8_0', true);
    await expect(mock.pinModel('nope', true)).rejects.toMatchObject({ code: 'notFound' });
    await mock.unloadModel('gemma-3-12b-it-q4_k_m');
    await expect(mock.unloadModel('gemma-3-12b-it-q4_k_m')).rejects.toMatchObject({ code: 'notFound' });
    await mock.setModelIdle('gemma-3-12b-it-q4_k_m', 300);
    await mock.setModelIdle('gemma-3-12b-it-q4_k_m', null);
    await expect(mock.setModelIdle('gemma-3-12b-it-q4_k_m', 5)).rejects.toMatchObject({ code: 'badRequest' });
    await expect(mock.setModelIdle('nope', null)).rejects.toMatchObject({ code: 'notFound' });
    expect(await mock.releaseMemory()).toBeGreaterThan(0);
  });

  it('refuses to load a model that will not fit', async () => {
    const mock = new MockBackend({ now: clock });
    const d = await mock.downloadModel('Qwen/Qwen3-32B-Instruct-GGUF', 'Qwen3-32B-Instruct-F16.gguf', null);
    for (let i = 0; i < 400; i++) mock.tick();
    const big = (await mock.localModels()).find((m) => m.file === d.name)!;
    await expect(mock.loadModel(big.id)).rejects.toMatchObject({ code: 'insufficientMemory' });
  });

  it('manages engine builds', async () => {
    const mock = new MockBackend({ now: clock });
    await expect(mock.removeBuild('cuda-13', 'b11669')).rejects.toMatchObject({ code: 'conflict' });
    await expect(mock.removeBuild('cuda-13', 'nope')).rejects.toMatchObject({ code: 'notFound' });
    await mock.selectBuild('vulkan');
    expect((await mock.engines())[0]!.builds.find((b) => b.active)?.id).toBe('vulkan');
    await mock.selectBuild('auto');
    expect((await mock.engines())[0]!.builds.find((b) => b.active)?.id).toBe('cuda-13');
    await expect(mock.selectBuild('metal')).rejects.toMatchObject({ code: 'notFound' });
    await mock.removeBuild('cpu', 'b11602');
    await mock.rollbackBuild();
    await expect(mock.rollbackBuild()).rejects.toMatchObject({ code: 'conflict' });
  });

  it('manages apps, keys and pairings', async () => {
    const mock = new MockBackend({ now: clock });
    const app = await mock.createApp('  My Tool ');
    expect(app.id).toBe('my-tool');
    await expect(mock.createApp('My Tool')).rejects.toMatchObject({ code: 'conflict' });
    await expect(mock.createApp('  ')).rejects.toMatchObject({ code: 'badRequest' });
    expect((await mock.createApp('!!!')).id).toMatch(/^app-/);
    const key = await mock.createKey('my-tool');
    expect(key.secret).toMatch(/^pw_[0-9a-f]{40}$/);
    expect(key.key.prefix).toBe(`${key.secret.slice(0, 7)}…`);
    await mock.revokeKey('my-tool', key.key.id);
    await expect(mock.createKey('nope')).rejects.toMatchObject({ code: 'notFound' });
    await expect(mock.revokeKey('nope', 'k')).rejects.toMatchObject({ code: 'notFound' });
    const perms = { ...app.permissions, tools: true };
    expect((await mock.setPermissions('my-tool', perms)).permissions.tools).toBe(true);
    await expect(mock.setPermissions('nope', perms)).rejects.toMatchObject({ code: 'notFound' });
    await mock.deleteApp('my-tool');
    expect((await mock.apps()).some((a) => a.id === 'my-tool')).toBe(false);
    await mock.approvePairing('pair-1', perms);
    expect((await mock.apps()).some((a) => a.id === 'obsidian-copilot')).toBe(true);
    await expect(mock.approvePairing('pair-1', perms)).rejects.toMatchObject({ code: 'notFound' });
    await mock.denyPairing('pair-x');
  });

  it('denies a pending pairing with an event', async () => {
    const mock = new MockBackend({ now: clock });
    const got: LiveUpdate[] = [];
    mock.subscribe((u) => got.push(u));
    await mock.denyPairing('pair-1');
    expect(got.some((u) => u.kind === 'event' && u.event.message.includes('Denied'))).toBe(true);
  });

  it('keeps request content only where it was kept', async () => {
    const mock = new MockBackend({ now: clock });
    const reqs = await mock.requests(10);
    const kept = reqs.find((r) => r.hasContent)!;
    const notKept = reqs.find((r) => !r.hasContent)!;
    expect((await mock.requestContent(kept.id)).request).toContain('messages');
    await expect(mock.requestContent(notKept.id)).rejects.toMatchObject({ code: 'notFound' });
    await expect(mock.requestContent('nope')).rejects.toMatchObject({ code: 'notFound' });
  });

  it('runs benchmarks, presets, plugins and jobs', async () => {
    const mock = new MockBackend({ now: clock });
    const r = await mock.runBenchmark('llama-3.2-3b-instruct-q5_k_m');
    expect(r.genTps).toBeGreaterThan(0);
    await expect(mock.runBenchmark('nope')).rejects.toMatchObject({ code: 'notFound' });
    expect((await mock.applyPreset('performance')).memory.maxHotModels).toBe(4);
    expect((await mock.applyPreset('custom')).memory.preset).toBe('custom');
    await mock.setPluginEnabled('pw-web-fetch', true);
    expect((await mock.plugins())[0]!.state).toBe('running');
    await mock.setPluginEnabled('pw-onnx-embed', false);
    expect((await mock.plugins())[2]!.state).toBe('quarantined');
    await expect(mock.setPluginEnabled('nope', true)).rejects.toMatchObject({ code: 'notFound' });
    await mock.cancelJob('job-42');
    await expect(mock.cancelJob('job-42')).rejects.toMatchObject({ code: 'notFound' });
    await mock.setServing(true);
    mock.tick();
    expect((await mock.overview()).throughput.genTps).toBe(0);
  });

  it('streams a chat reply word by word and stops on cancel', async () => {
    vi.useFakeTimers();
    try {
      const mock = new MockBackend({ now: clock });
      const events: PlaygroundEvent[] = [];
      mock.chat({ modelId: 'llama-3.2-3b-instruct-q5_k_m', messages: [{ role: 'user', content: 'hi' }], temperature: 0.7, maxTokens: 512 }, (e) => events.push(e));
      await vi.runAllTimersAsync();
      expect(events.filter((e) => e.kind === 'delta').length).toBeGreaterThan(10);
      expect(events[events.length - 1]).toMatchObject({ kind: 'done', coldStartMs: null });

      const cut: PlaygroundEvent[] = [];
      const stop = mock.chat({ modelId: 'gemma-3-12b-it-q4_k_m', messages: [{ role: 'user', content: 'hi' }], temperature: 0.7, maxTokens: 512 }, (e) => cut.push(e));
      await vi.advanceTimersByTimeAsync(0);
      await vi.advanceTimersByTimeAsync(0);
      stop();
      await vi.runAllTimersAsync();
      expect(cut.some((e) => e.kind === 'done')).toBe(false);

      const missing: PlaygroundEvent[] = [];
      mock.chat({ modelId: 'nope', messages: [], temperature: 0, maxTokens: 1 }, (e) => missing.push(e));
      await vi.runAllTimersAsync();
      expect(missing).toEqual([{ kind: 'error', error: { code: 'notFound', message: 'No model nope.' } }]);

      const failing = new MockBackend({ failing: ['chat'], streamDelayMs: 5 });
      const failed: PlaygroundEvent[] = [];
      failing.chat({ modelId: 'x', messages: [], temperature: 0, maxTokens: 1 }, (e) => failed.push(e));
      await vi.runAllTimersAsync();
      expect(failed[0]!.kind).toBe('error');

      const slow = new MockBackend({ streamDelayMs: 5 });
      const s: PlaygroundEvent[] = [];
      slow.chat({ modelId: 'qwen3-8b-instruct-q4_k_m', messages: [{ role: 'user', content: 'hi' }], temperature: 0, maxTokens: 4 }, (e) => s.push(e));
      await vi.runAllTimersAsync();
      expect(s.filter((e) => e.kind === 'delta').length).toBe(4);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('mock data', () => {
  it('is deterministic', () => {
    const a = prng(1);
    const b = prng(1);
    expect([a(), a(), a()]).toEqual([b(), b(), b()]);
    expect(demoHistory('24h', NOW)).toEqual(demoHistory('24h', NOW));
    expect(demoRequests(NOW, 20)).toEqual(demoRequests(NOW, 20));
  });

  it('judges fit by size and builds repos and searches', () => {
    expect(demoFit(1 * GiB).verdict).toBe('fits');
    expect(demoFit(6 * GiB).verdict).toBe('tight');
    expect(demoFit(15 * GiB).offload).toBe('partial');
    expect(demoFit(40 * GiB).verdict).toBe('wontFit');
    const repo = demoRepo('Qwen/Qwen3-32B-Instruct-GGUF', []);
    expect(repo.variants.find((v) => v.quant === 'F16')!.parts).toBe(2);
    expect(repo.license).toBe('apache-2.0');
    expect(demoRepo('bartowski/Meta-Llama-3.1-8B-Instruct-GGUF', []).gated).toBe(true);
    expect(demoRepo('nomic-ai/nomic-embed-text-v1.5-GGUF', []).variants.length).toBe(8);
    expect(demoSearch('', NOW).length).toBe(12);
    expect(presetMemory('lowMemory', presetMemory('balanced', presetMemory('custom', { preset: 'balanced' } as never))).maxHotModels).toBe(1);
  });
});
