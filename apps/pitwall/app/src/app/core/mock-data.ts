/**
 * Demo data for the mock backend: a 32 GB Windows box with an RTX 4070 (12 GB), a handful of local models and the
 * suite's apps. Everything is generated from a seeded PRNG relative to a given "now", so screenshots and tests are
 * deterministic. Browser development and UI tests only: this is never the reference for behaviour.
 */
import type {
  AppInfo,
  AppPermissions,
  BenchmarkResult,
  Engine,
  EventKind,
  Fit,
  FitVerdict,
  HfModel,
  HfRepo,
  History,
  HistoryPoint,
  HistoryRange,
  KeyedSeries,
  LoadedModel,
  LocalModel,
  PairingRequest,
  PitwallEvent,
  QuantVariant,
  ReleaseReason,
  RequestRecord,
  Settings,
  Severity,
} from './api.types';

export const GiB = 1024 ** 3;
export const MiB = 1024 ** 2;
export const RAM_TOTAL = 32 * GiB;
export const VRAM_TOTAL = 12 * GiB;
export const GPU_NAME = 'NVIDIA GeForce RTX 4070';
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** Mulberry32: a tiny deterministic PRNG. */
export function prng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export const APP_IDS = ['imt', 'pitcrew', 'spotter', 'dyno', 'continue'] as const;

const ALL_PERMS: AppPermissions = {
  models: null,
  maxPriority: 'interactive',
  tools: true,
  extendedApis: true,
  limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: 4 },
};

export function demoApps(now: number): AppInfo[] {
  const app = (id: string, name: string, suite: boolean, connected: boolean, today: AppInfo['today'], perms: Partial<AppPermissions> = {}, keys = 1): AppInfo => ({
    id,
    name,
    suite,
    connected,
    lastSeenAt: connected ? now - 4_000 : now - 3 * HOUR,
    keys: Array.from({ length: keys }, (_, i) => ({
      id: `${id}-k${i + 1}`,
      prefix: `pw_${id.slice(0, 2)}${(i + 7).toString(16)}${'3f9a'.slice(i)}…`,
      createdAt: now - (40 - i * 9) * DAY,
      lastUsedAt: connected ? now - 4_000 - i * HOUR : now - 3 * HOUR,
    })),
    permissions: { ...ALL_PERMS, ...perms },
    today,
  });
  return [
    app('imt', 'IMT', true, true, { requests: 1284, promptTokens: 1_942_310, completionTokens: 286_940, busyMs: 2_710_000, errors: 3 }),
    app('pitcrew', 'Pitcrew', true, true, { requests: 212, promptTokens: 148_220, completionTokens: 31_400, busyMs: 402_000, errors: 0 }, { maxPriority: 'background', tools: false }),
    app('spotter', 'Spotter', true, true, { requests: 3460, promptTokens: 905_000, completionTokens: 12_400, busyMs: 1_120_000, errors: 12 }, {
      maxPriority: 'batch',
      models: ['nomic-embed-text-v1.5-f16', 'bge-reranker-v2-m3-q8_0'],
      limits: { requestsPerMin: 600, tokensPerDay: 5_000_000, maxConcurrent: 2 },
    }),
    app('dyno', 'Dyno', true, false, { requests: 48, promptTokens: 61_200, completionTokens: 18_900, busyMs: 96_000, errors: 1 }, { maxPriority: 'background' }),
    app('continue', 'Continue (VS Code)', false, true, { requests: 391, promptTokens: 702_600, completionTokens: 98_120, busyMs: 840_000, errors: 0 }, {
      extendedApis: false,
      limits: { requestsPerMin: 60, tokensPerDay: 2_000_000, maxConcurrent: 2 },
    }, 2),
  ];
}

export function demoPairings(now: number): PairingRequest[] {
  return [
    {
      id: 'pair-1',
      appId: 'obsidian-copilot',
      appName: 'Obsidian Copilot',
      code: '482913',
      requestedAt: now - 40_000,
      permissions: { models: null, maxPriority: 'interactive', tools: false, extendedApis: false, limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 1 } },
    },
  ];
}

function fit(verdict: FitVerdict, needRam: number, needVram: number, offload: Fit['offload'], message: string): Fit {
  return { verdict, needRam, needVram, haveRam: 14.6 * GiB, haveVram: 4.1 * GiB, offload, message };
}

/** Fit of a model of `size` bytes on the demo box (mock only: the real verdict comes from the server). */
export function demoFit(size: number): Fit {
  const need = size * 1.18 + 600 * MiB;
  if (need < 3.6 * GiB) return fit('fits', 600 * MiB, need, 'all', 'Fits on the GPU with room for context.');
  if (need < 11.2 * GiB) return fit('tight', need - 3.2 * GiB, 3.2 * GiB, 'partial', 'Fits with part of the layers in RAM; another hot model may be released.');
  if (need < 26 * GiB) return fit('tight', need, 0, 'partial', 'Fits in RAM with partial GPU offload; expect slower generation.');
  return fit('wontFit', need, 0, 'none', `Needs about ${(need / GiB).toFixed(0)} GB; this machine has 32 GB.`);
}

export function demoLocalModels(now: number): LocalModel[] {
  const m = (p: Partial<LocalModel> & Pick<LocalModel, 'id' | 'repo' | 'file' | 'name' | 'sizeBytes'>): LocalModel => ({
    quant: null,
    paramsB: null,
    contextMax: null,
    architecture: null,
    capabilities: ['chat'],
    projector: null,
    addedAt: now - 20 * DAY,
    lastUsedAt: now - HOUR,
    loaded: false,
    pinned: false,
    idleSecs: null,
    fit: demoFit(p.sizeBytes),
    ...p,
  });
  return [
    m({ id: 'qwen3-8b-instruct-q4_k_m', repo: 'Qwen/Qwen3-8B-Instruct-GGUF', file: 'Qwen3-8B-Instruct-Q4_K_M.gguf', name: 'Qwen3 8B Instruct', quant: 'Q4_K_M', sizeBytes: 5.03 * GiB, paramsB: 8.2, contextMax: 32768, architecture: 'qwen3', capabilities: ['chat', 'tools', 'json'], loaded: true, lastUsedAt: now - 48_000, addedAt: now - 34 * DAY }),
    m({ id: 'llama-3.2-3b-instruct-q5_k_m', repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF', file: 'Llama-3.2-3B-Instruct-Q5_K_M.gguf', name: 'Llama 3.2 3B Instruct', quant: 'Q5_K_M', sizeBytes: 2.32 * GiB, paramsB: 3.2, contextMax: 131072, architecture: 'llama', capabilities: ['chat', 'tools'], loaded: true, lastUsedAt: now - 2_000, idleSecs: 300 }),
    m({ id: 'nomic-embed-text-v1.5-f16', repo: 'nomic-ai/nomic-embed-text-v1.5-GGUF', file: 'nomic-embed-text-v1.5.f16.gguf', name: 'nomic-embed-text v1.5', quant: 'F16', sizeBytes: 262 * MiB, paramsB: 0.14, contextMax: 8192, architecture: 'nomic-bert', capabilities: ['embeddings'], loaded: true, pinned: true, lastUsedAt: now - 9_000 }),
    m({ id: 'gemma-3-12b-it-q4_k_m', repo: 'unsloth/gemma-3-12b-it-GGUF', file: 'gemma-3-12b-it-Q4_K_M.gguf', name: 'Gemma 3 12B', quant: 'Q4_K_M', sizeBytes: 6.79 * GiB, paramsB: 12.2, contextMax: 131072, architecture: 'gemma3', capabilities: ['chat', 'vision', 'json'], projector: 'mmproj-F16.gguf', lastUsedAt: now - 2 * DAY, idleSecs: 120 }),
    m({ id: 'bge-reranker-v2-m3-q8_0', repo: 'gpustack/bge-reranker-v2-m3-GGUF', file: 'bge-reranker-v2-m3-Q8_0.gguf', name: 'BGE reranker v2 m3', quant: 'Q8_0', sizeBytes: 606 * MiB, paramsB: 0.57, contextMax: 8192, architecture: 'xlm-roberta', capabilities: ['rerank'], lastUsedAt: now - 5 * HOUR }),
    m({ id: 'qwen2.5-coder-1.5b-q8_0', repo: 'Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF', file: 'qwen2.5-coder-1.5b-instruct-q8_0.gguf', name: 'Qwen2.5 Coder 1.5B', quant: 'Q8_0', sizeBytes: 1.76 * GiB, paramsB: 1.5, contextMax: 32768, architecture: 'qwen2', capabilities: ['chat'], lastUsedAt: now - 41 * DAY, addedAt: now - 80 * DAY }),
  ];
}

export function demoLoaded(now: number, idleSecs = 600): LoadedModel[] {
  return [
    { modelId: 'qwen3-8b-instruct-q4_k_m', name: 'Qwen3 8B Instruct', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: 0.9 * GiB, vramBytes: 6.1 * GiB, contextTokens: 16384, pinned: false, activeRequests: 0, loadedAt: now - 52 * MIN, lastUsedAt: now - 48_000, idleReleaseAt: now - 48_000 + idleSecs * 1000, coldStartMs: 3_420, capabilities: ['chat', 'tools', 'json'] },
    { modelId: 'llama-3.2-3b-instruct-q5_k_m', name: 'Llama 3.2 3B Instruct', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'busy', ramBytes: 0.4 * GiB, vramBytes: 2.9 * GiB, contextTokens: 8192, pinned: false, activeRequests: 2, loadedAt: now - 14 * MIN, lastUsedAt: now - 2_000, idleReleaseAt: null, coldStartMs: 1_610, capabilities: ['chat', 'tools'] },
    { modelId: 'nomic-embed-text-v1.5-f16', name: 'nomic-embed-text v1.5', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: 0.12 * GiB, vramBytes: 0.33 * GiB, contextTokens: 8192, pinned: true, activeRequests: 0, loadedAt: now - 5 * HOUR, lastUsedAt: now - 9_000, idleReleaseAt: null, coldStartMs: 420, capabilities: ['embeddings'] },
  ];
}

export function demoEngines(now: number): Engine[] {
  return [
    {
      kind: 'llama.cpp',
      name: 'llama.cpp',
      description: 'GGUF models on CPU, CUDA, Vulkan and Metal. Chat, tools, vision, embeddings and reranking.',
      availability: { state: 'available' },
      buildChoice: 'auto',
      builds: [
        { id: 'cuda-13', tag: 'b11669', label: 'CUDA 13 (NVIDIA)', sizeBytes: 412 * MiB, selfTest: 'passed', selfTestMessage: 'CUDA0: NVIDIA GeForce RTX 4070 (12282 MiB)', devices: ['CUDA0: NVIDIA GeForce RTX 4070'], active: true, outdated: true },
        { id: 'vulkan', tag: 'b11669', label: 'Vulkan', sizeBytes: 38 * MiB, selfTest: 'passed', selfTestMessage: 'Vulkan0: NVIDIA GeForce RTX 4070', devices: ['Vulkan0: NVIDIA GeForce RTX 4070'], active: false, outdated: true },
        { id: 'cpu', tag: 'b11602', label: 'CPU (AVX2)', sizeBytes: 24 * MiB, selfTest: 'notRun', selfTestMessage: null, devices: ['CPU'], active: false, outdated: true },
      ],
      catalog: [
        { id: 'cuda-13', label: 'CUDA 13 (NVIDIA)', tag: 'b11702', downloadBytes: 398 * MiB, suggested: true, installed: false },
        { id: 'cuda-12', label: 'CUDA 12 (NVIDIA, older drivers)', tag: 'b11702', downloadBytes: 381 * MiB, suggested: false, installed: false },
        { id: 'vulkan', label: 'Vulkan (any GPU)', tag: 'b11702', downloadBytes: 37 * MiB, suggested: false, installed: false },
        { id: 'cpu', label: 'CPU (AVX2)', tag: 'b11702', downloadBytes: 23 * MiB, suggested: false, installed: false },
      ],
      processes: [
        { pid: 18244, modelId: 'qwen3-8b-instruct-q4_k_m', health: 'healthy', restarts: 0, rssBytes: 1.1 * GiB, startedAt: now - 52 * MIN },
        { pid: 20112, modelId: 'llama-3.2-3b-instruct-q5_k_m', health: 'healthy', restarts: 1, rssBytes: 0.6 * GiB, startedAt: now - 14 * MIN },
        { pid: 9960, modelId: 'nomic-embed-text-v1.5-f16', health: 'healthy', restarts: 0, rssBytes: 0.21 * GiB, startedAt: now - 5 * HOUR },
      ],
      rollbackTo: 'b11602',
    },
    { kind: 'whisper.cpp', name: 'whisper.cpp', description: 'Speech to text (OpenAI-compatible /v1/audio/transcriptions).', availability: { state: 'notInstalled', reason: 'Planned for a later version: transcription is not wired to the router yet.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
    { kind: 'mlx', name: 'MLX', description: "Apple's array framework for Apple silicon.", availability: { state: 'notAvailable', reason: 'MLX runs on Apple silicon only; this is a Windows PC.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
    { kind: 'vllm', name: 'vLLM', description: 'High-throughput serving with paged attention.', availability: { state: 'notAvailable', reason: 'vLLM needs Linux with a CUDA GPU; Windows is not supported.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
  ];
}

export function demoSettings(): Settings {
  return {
    general: { startAtLogin: true, theme: 'system', keepServingWhenClosed: true },
    server: { port: 11500, maxRequestBytes: 32 * MiB, requestTimeoutSecs: 600, maxConcurrent: 4, allowKeylessLocal: false },
    memory: {
      preset: 'balanced',
      idleUnloadSecs: 600,
      deepIdleSecs: 1800,
      releaseContextFirst: true,
      pressureAvailablePercent: 15,
      minFreeBytes: 4 * GiB,
      maxHotModels: 3,
      hotCapBytes: null,
      yieldToForeground: true,
      notifyOnRelease: true,
    },
    network: { offline: false, lanEnabled: false, lanPort: 11501 },
    privacy: { keepRequestContent: false, metricsRetentionDays: 30 },
  };
}

/** The values each preset sets (mock only; the server owns the real ones). */
export function presetMemory(preset: Settings['memory']['preset'], current: Settings['memory']): Settings['memory'] {
  switch (preset) {
    case 'lowMemory':
      return { ...current, preset, idleUnloadSecs: 120, deepIdleSecs: 600, releaseContextFirst: true, pressureAvailablePercent: 25, minFreeBytes: 6 * GiB, maxHotModels: 1, hotCapBytes: 8 * GiB };
    case 'balanced':
      return { ...current, preset, idleUnloadSecs: 600, deepIdleSecs: 1800, releaseContextFirst: true, pressureAvailablePercent: 15, minFreeBytes: 4 * GiB, maxHotModels: 3, hotCapBytes: null };
    case 'performance':
      return { ...current, preset, idleUnloadSecs: 3600, deepIdleSecs: 4 * 3600, releaseContextFirst: false, pressureAvailablePercent: 8, minFreeBytes: 2 * GiB, maxHotModels: 4, hotCapBytes: null };
    default:
      return { ...current, preset };
  }
}

const HF_MODELS: [string, number, number, string[], string][] = [
  ['Qwen/Qwen3-8B-Instruct-GGUF', 1_204_311, 1840, ['gguf', 'qwen3', 'conversational'], 'text-generation'],
  ['Qwen/Qwen3-14B-Instruct-GGUF', 612_004, 990, ['gguf', 'qwen3', 'conversational'], 'text-generation'],
  ['Qwen/Qwen3-32B-Instruct-GGUF', 288_190, 760, ['gguf', 'qwen3'], 'text-generation'],
  ['Qwen/Qwen2.5-Coder-7B-Instruct-GGUF', 902_118, 1320, ['gguf', 'code'], 'text-generation'],
  ['unsloth/gemma-3-12b-it-GGUF', 740_225, 1102, ['gguf', 'gemma3', 'vision'], 'image-text-to-text'],
  ['unsloth/gemma-3-27b-it-GGUF', 330_910, 840, ['gguf', 'gemma3', 'vision'], 'image-text-to-text'],
  ['bartowski/Llama-3.2-3B-Instruct-GGUF', 2_210_400, 2210, ['gguf', 'llama'], 'text-generation'],
  ['bartowski/Meta-Llama-3.1-8B-Instruct-GGUF', 1_840_300, 2040, ['gguf', 'llama'], 'text-generation'],
  ['bartowski/Mistral-Small-3.2-24B-Instruct-GGUF', 205_300, 480, ['gguf', 'mistral'], 'text-generation'],
  ['microsoft/Phi-4-mini-instruct-gguf', 410_200, 612, ['gguf', 'phi'], 'text-generation'],
  ['nomic-ai/nomic-embed-text-v1.5-GGUF', 1_502_990, 610, ['gguf', 'embeddings'], 'feature-extraction'],
  ['gpustack/bge-reranker-v2-m3-GGUF', 120_400, 88, ['gguf', 'reranker'], 'text-classification'],
];

export function demoSearch(query: string, now: number): HfModel[] {
  const q = query.trim().toLowerCase();
  return HF_MODELS.filter(([repo]) => !q || repo.toLowerCase().includes(q)).map(([repo, downloads, likes, tags, pipeline], i) => ({
    repo,
    author: repo.split('/')[0]!,
    downloads,
    likes,
    updatedAt: now - (i + 2) * 3 * DAY,
    tags,
    pipeline,
  }));
}

const QUANTS: [string, number][] = [
  ['Q2_K', 0.37],
  ['Q3_K_M', 0.49],
  ['Q4_K_S', 0.57],
  ['Q4_K_M', 0.61],
  ['Q5_K_M', 0.71],
  ['Q6_K', 0.82],
  ['Q8_0', 1.06],
  ['F16', 2.0],
];

function paramsOf(repo: string): number {
  const m = /(\d+(?:\.\d+)?)B/i.exec(repo);
  return m ? Number(m[1]) : 0.5;
}

export function demoRepo(repo: string, local: LocalModel[]): HfRepo {
  const params = paramsOf(repo);
  const base = repo.split('/')[1]!.replace(/-GGUF$/i, '');
  const variants: QuantVariant[] = QUANTS.map(([quant, bpp]) => {
    const size = params * 1e9 * bpp;
    const parts = size > 40 * GiB ? 2 : 1;
    const file = parts > 1 ? `${base}-${quant}-00001-of-0000${parts}.gguf` : `${base}-${quant}.gguf`;
    return { file, quant, sizeBytes: size, parts, sha256: null, fit: demoFit(size), downloaded: local.some((l) => l.repo === repo && l.quant === quant) };
  });
  const vision = /gemma-3|vision|vl/i.test(repo);
  return {
    repo,
    variants,
    projectors: vision ? [{ file: 'mmproj-F16.gguf', quant: 'F16', sizeBytes: 854 * MiB, parts: 1, sha256: null, fit: demoFit(854 * MiB), downloaded: false }] : [],
    license: /gemma/i.test(repo) ? 'gemma' : /llama/i.test(repo) ? 'llama3.2' : 'apache-2.0',
    gated: /meta-llama/i.test(repo),
  };
}

// ---- Histories ---------------------------------------------------------------------------------------------------

const RANGES: Record<HistoryRange, { span: number; points: number }> = {
  '1h': { span: HOUR, points: 120 },
  '24h': { span: DAY, points: 144 },
  '7d': { span: 7 * DAY, points: 168 },
  '30d': { span: 30 * DAY, points: 180 },
};

// Keyed series use ids (app ids, model ids); the UI shows names.
const MODEL_KEYS = ['qwen3-8b-instruct-q4_k_m', 'llama-3.2-3b-instruct-q5_k_m', 'nomic-embed-text-v1.5-f16', 'gemma-3-12b-it-q4_k_m'];
const APP_KEYS = ['imt', 'spotter', 'continue', 'pitcrew', 'dyno'];

export function demoHistory(range: HistoryRange, now: number, seed = 7): History {
  const rnd = prng(seed + range.length * 31 + range.charCodeAt(0));
  const { span, points: n } = RANGES[range];
  const stepMs = span / n;
  const start = now - span;
  const points: HistoryPoint[] = [];
  const byApp: KeyedSeries[] = APP_KEYS.map((key) => ({ key, values: [] }));
  const byModel: KeyedSeries[] = MODEL_KEYS.map((key) => ({ key, values: [] }));
  const events: PitwallEvent[] = [];
  let id = 10_000 + range.length * 1000;

  // Model residency as steps: which models are hot in each segment of the range.
  const segments = 7;
  const hot: boolean[][] = Array.from({ length: segments }, (_, s) => [s !== 1 && s !== 4, s >= 2 && s !== 5, true, s === 3]);
  hot[segments - 1] = [true, true, true, false];
  const sizes = [7.0 * GiB, 3.3 * GiB, 0.45 * GiB, 8.1 * GiB];
  const vramShare = [0.87, 0.88, 0.73, 0.4];

  for (let i = 0; i < n; i++) {
    const at = start + (i + 1) * stepMs;
    const seg = Math.min(segments - 1, Math.floor((i / n) * segments));
    const h = new Date(at).getHours();
    const daytime = range === '1h' ? 1 : 0.35 + 0.65 * Math.max(0, Math.sin(((h - 7) / 14) * Math.PI));
    const active = hot[seg]!;
    const modelRam = active.reduce((sum, on, k) => sum + (on ? sizes[k]! * (1 - vramShare[k]!) : 0), 0);
    const modelVram = active.reduce((sum, on, k) => sum + (on ? sizes[k]! * vramShare[k]! : 0), 0);
    const other = 10.4 * GiB + Math.sin(i / 9) * 0.8 * GiB + rnd() * 0.5 * GiB + daytime * 1.6 * GiB;
    const load = daytime * (0.55 + rnd() * 0.45);
    const reqs = Math.round(load * (range === '1h' ? 14 : range === '24h' ? 120 : range === '7d' ? 720 : 2600));
    points.push({
      at,
      ramUsed: other + modelRam + 1.1 * GiB,
      ramTotal: RAM_TOTAL,
      vramUsed: 0.9 * GiB + modelVram,
      vramTotal: VRAM_TOTAL,
      pitwallRss: 0.18 * GiB + modelRam + active.filter(Boolean).length * 0.12 * GiB,
      cpuPercent: 6 + load * 30 + rnd() * 6,
      promptTps: load > 0.1 ? 380 + load * 900 + rnd() * 160 : 0,
      genTps: load > 0.1 ? 34 + load * 22 + rnd() * 6 : 0,
      p50Ms: reqs ? 180 + rnd() * 90 + (1 - daytime) * 60 : null,
      p95Ms: reqs ? 820 + rnd() * 600 + (rnd() > 0.93 ? 2600 : 0) : null,
      requests: reqs,
    });
    const shares = [0.42, 0.33, 0.14, 0.08, 0.03];
    byApp.forEach((s, k) => s.values.push(Math.round(reqs * shares[k]! * (0.8 + rnd() * 0.4))));
    const mShares = [0.5, 0.2, 0.27, 0.03];
    byModel.forEach((s, k) => s.values.push(active[k] ? Math.round(reqs * mShares[k]! * (0.8 + rnd() * 0.4)) : 0));
  }

  // Events at segment boundaries where residency changed.
  const reasons: ReleaseReason[] = ['idle', 'pressure', 'manual', 'evicted', 'deepIdle', 'idle'];
  const names = ['Qwen3 8B Instruct', 'Llama 3.2 3B Instruct', 'nomic-embed-text v1.5', 'Gemma 3 12B'];
  const ids = ['qwen3-8b-instruct-q4_k_m', 'llama-3.2-3b-instruct-q5_k_m', 'nomic-embed-text-v1.5-f16', 'gemma-3-12b-it-q4_k_m'];
  for (let s = 1; s < segments; s++) {
    const at = start + (s / segments) * span;
    for (let k = 0; k < 4; k++) {
      const was = hot[s - 1]![k]!;
      const is = hot[s]![k]!;
      if (was === is) continue;
      const reason = is ? null : reasons[(s + k) % reasons.length]!;
      events.push({
        id: id++,
        at: at + k * stepMs * 0.3,
        kind: is ? 'modelLoaded' : 'modelUnloaded',
        severity: reason === 'pressure' ? 'warn' : 'info',
        message: is ? `Loaded ${names[k]} (${(1.2 + k).toFixed(1)} s cold start)` : `Released ${names[k]}: ${reason === 'pressure' ? 'available RAM fell below 15 %' : reason === 'manual' ? 'released by you' : reason === 'deepIdle' ? 'deep idle' : reason === 'evicted' ? 'made room for another model' : 'idle for 10 min'}`,
        modelId: ids[k]!,
        appId: null,
        reason,
      });
    }
  }
  events.push({ id: id++, at: start + span * 0.62, kind: 'engineRestarted', severity: 'warn', message: 'llama.cpp (Llama 3.2 3B) stopped responding and was restarted', modelId: ids[1]!, appId: null, reason: null });
  events.sort((a, b) => a.at - b.at);
  return { range, stepMs, points, requestsByApp: byApp, requestsByModel: byModel, events };
}

export function demoEvents(now: number): PitwallEvent[] {
  const e = (minsAgo: number, kind: EventKind, severity: Severity, message: string, extra: Partial<PitwallEvent> = {}): PitwallEvent => ({
    id: 0,
    at: now - minsAgo * MIN,
    kind,
    severity,
    message,
    modelId: null,
    appId: null,
    reason: null,
    ...extra,
  });
  const list = [
    e(0.7, 'pairing', 'info', 'Obsidian Copilot asks to pair (code 482 913)', { appId: 'obsidian-copilot' }),
    e(3, 'contextReleased', 'info', 'Released the context of Qwen3 8B Instruct after 2 min idle (1.2 GB)', { modelId: 'qwen3-8b-instruct-q4_k_m', reason: 'idle' }),
    e(14, 'modelLoaded', 'info', 'Loaded Llama 3.2 3B Instruct for Continue (1.6 s cold start)', { modelId: 'llama-3.2-3b-instruct-q5_k_m', appId: 'continue' }),
    e(22, 'modelUnloaded', 'warn', 'Released Gemma 3 12B: available RAM fell below 15 %', { modelId: 'gemma-3-12b-it-q4_k_m', reason: 'pressure' }),
    e(31, 'requestFailed', 'error', 'Spotter: 429 rate limit (600 requests/min)', { appId: 'spotter' }),
    e(48, 'download', 'info', 'Downloaded bge-reranker-v2-m3 Q8_0 (606 MB, verified)'),
    e(52, 'modelLoaded', 'info', 'Loaded Qwen3 8B Instruct for IMT (3.4 s cold start)', { modelId: 'qwen3-8b-instruct-q4_k_m', appId: 'imt' }),
    e(67, 'engineRestarted', 'warn', 'llama.cpp (Llama 3.2 3B) stopped responding and was restarted', { modelId: 'llama-3.2-3b-instruct-q5_k_m' }),
    e(95, 'modelUnloaded', 'info', 'Released Qwen3 8B Instruct: idle for 10 min', { modelId: 'qwen3-8b-instruct-q4_k_m', reason: 'idle' }),
    e(130, 'presetChanged', 'info', 'Memory preset set to Balanced'),
    e(180, 'modelUnloaded', 'info', 'Released Gemma 3 12B: yielded to a full-screen app', { modelId: 'gemma-3-12b-it-q4_k_m', reason: 'yield' }),
    e(240, 'servingChanged', 'info', 'Serving resumed'),
    e(300, 'engineStarted', 'info', 'llama.cpp cuda-13 b11669 passed its self-test'),
  ];
  return list.map((x, i) => ({ ...x, id: 1000 - i }));
}

const ENDPOINTS: [RequestRecord['api'], string][] = [
  ['openai', '/v1/chat/completions'],
  ['openai', '/v1/embeddings'],
  ['anthropic', '/v1/messages'],
  ['ollama', '/api/chat'],
  ['extended', '/x/v1/rerank'],
  ['openai', '/v1/completions'],
];

export function demoRequests(now: number, count = 400, seed = 3): RequestRecord[] {
  const rnd = prng(seed);
  const out: RequestRecord[] = [];
  let at = now - 1_500;
  for (let i = 0; i < count; i++) {
    const appIdx = Math.floor(rnd() ** 1.4 * 5);
    const appId = APP_IDS[appIdx]!;
    let [api, endpoint] = ENDPOINTS[Math.floor(rnd() * 4)]!;
    let modelId: string = rnd() > 0.35 ? 'qwen3-8b-instruct-q4_k_m' : 'llama-3.2-3b-instruct-q5_k_m';
    if (appId === 'spotter') {
      [api, endpoint] = rnd() > 0.3 ? ENDPOINTS[1]! : ENDPOINTS[4]!;
      modelId = endpoint.includes('rerank') ? 'bge-reranker-v2-m3-q8_0' : 'nomic-embed-text-v1.5-f16';
    }
    if (appId === 'continue') [api, endpoint] = rnd() > 0.5 ? ENDPOINTS[3]! : ENDPOINTS[5]!;
    const embedding = endpoint.includes('embed') || endpoint.includes('rerank');
    const fail = rnd();
    const status = fail > 0.975 ? 429 : fail > 0.965 ? 500 : fail > 0.955 ? 400 : 200;
    const cold = rnd() > 0.96 ? Math.round(1400 + rnd() * 2400) : null;
    const queuedMs = Math.round(rnd() ** 3 * 900);
    const ttft = embedding || status !== 200 ? null : Math.round(120 + rnd() * 300 + (cold ?? 0));
    const completion = embedding || status !== 200 ? 0 : Math.round(40 + rnd() * 600);
    out.push({
      id: `req-${(count - i).toString(36).padStart(4, '0')}`,
      at,
      appId,
      api,
      endpoint,
      modelId: status === 400 ? null : modelId,
      chosenBy: rnd() > 0.6 ? 'explicit' : rnd() > 0.4 ? 'capabilities' : 'default',
      priority: appId === 'spotter' ? 'batch' : appId === 'pitcrew' || appId === 'dyno' ? 'background' : 'interactive',
      status,
      stream: !embedding && rnd() > 0.3,
      queuedMs,
      ttftMs: ttft,
      totalMs: Math.round(queuedMs + (ttft ?? 40) + completion * (18 + rnd() * 10) + (embedding ? 20 + rnd() * 80 : 0)),
      coldStartMs: cold,
      promptTokens: Math.round(embedding ? 64 + rnd() * 400 : 200 + rnd() * 3800),
      completionTokens: completion,
      error: status === 429 ? 'Rate limit: 600 requests/min' : status === 500 ? 'Engine stopped responding' : status === 400 ? 'Unknown model "gpt-4o"' : null,
      hasContent: i % 3 !== 2,
    });
    at -= Math.round(2_000 + rnd() * 25_000);
  }
  return out;
}

export function demoBenchmarks(now: number): BenchmarkResult[] {
  return [
    { modelId: 'qwen3-8b-instruct-q4_k_m', build: 'cuda-13 · b11669', at: now - 2 * DAY, promptTps: 2140, genTps: 61.8, ttftMs: 212, contextTokens: 4096 },
    { modelId: 'llama-3.2-3b-instruct-q5_k_m', build: 'cuda-13 · b11669', at: now - 2 * DAY, promptTps: 4380, genTps: 118.4, ttftMs: 96, contextTokens: 4096 },
    { modelId: 'gemma-3-12b-it-q4_k_m', build: 'cuda-13 · b11669', at: now - 6 * DAY, promptTps: 1210, genTps: 38.9, ttftMs: 384, contextTokens: 4096 },
    { modelId: 'qwen2.5-coder-1.5b-q8_0', build: 'vulkan · b11669', at: now - 12 * DAY, promptTps: 5120, genTps: 141.2, ttftMs: 71, contextTokens: 4096 },
    { modelId: 'qwen3-8b-instruct-q4_k_m', build: 'vulkan · b11669', at: now - 3 * DAY, promptTps: 1490, genTps: 52.3, ttftMs: 268, contextTokens: 4096 },
  ];
}

export const DEMO_REPLY =
  'Pitwall keeps models hot only while they are useful. When a model has been idle for the configured time it first releases the context (the KV cache), and later the weights, so your games and IDE get the memory back. Under memory pressure it releases the least recently used model at once, and pinned models stay loaded until you unpin them.';
