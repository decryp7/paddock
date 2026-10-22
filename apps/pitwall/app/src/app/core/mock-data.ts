/**
 * Demo data for the mock backend: a 32 GB Windows box with an RTX 4070 (12 GB), a handful of local models and the
 * suite's apps. Everything is generated from a seeded PRNG relative to a given "now", so screenshots and tests are
 * deterministic. Browser development and UI tests only: this is never the reference for behaviour.
 */
import type {
  ModelPreset,
  PacksInfo,
  Recommendation,
  AppDetail,
  AppInfo,
  AppLimits,
  AppPermissions,
  AppUsageSeries,
  BenchmarkResult,
  BuildCapability,
  Capability,
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
  LimitStatus,
  LoadedModel,
  LocalModel,
  PairingRequest,
  PitwallEvent,
  QuantVariant,
  ReleaseReason,
  RequestRecord,
  Settings,
  Severity,
  UsageCounters,
  UsageRange,
  UsageSeries,
  SlotFit,
  UpdateNotice,
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

export const APP_IDS = ['debrief', 'pitcrew', 'spotter', 'dyno', 'nightly-report'] as const;

const ALL_PERMS: AppPermissions = {
  models: null,
  maxPriority: 'interactive',
  tools: true,
  extendedApis: true,
  cacheResponses: false,
  limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: 4, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null },
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
    app('debrief', 'Debrief', true, true, { requests: 1284, promptTokens: 1_942_310, completionTokens: 286_940, busyMs: 2_710_000, errors: 3, peakMemoryBytes: 7.4 * GiB, cacheHits: 212, cacheMisses: 388, prefixTokens: 1_180_000, savedTokens: 96_400 }, { cacheResponses: true }),
    app('pitcrew', 'Pitcrew', true, true, { requests: 212, promptTokens: 148_220, completionTokens: 31_400, busyMs: 402_000, errors: 0, peakMemoryBytes: 3.6 * GiB, cacheHits: 0, cacheMisses: 0, prefixTokens: 61_000, savedTokens: 0 }, { maxPriority: 'background', tools: false }),
    app('spotter', 'Spotter', true, true, { requests: 3460, promptTokens: 905_000, completionTokens: 12_400, busyMs: 1_120_000, errors: 12, peakMemoryBytes: 0.5 * GiB, cacheHits: 2_310, cacheMisses: 1_150, prefixTokens: 0, savedTokens: 601_000 }, {
      maxPriority: 'batch',
      models: ['nomic-embed-text-v1.5-f16', 'bge-reranker-v2-m3-q8_0'],
      limits: { requestsPerMin: 600, tokensPerDay: 5_000_000, maxConcurrent: 2, requestsPerDay: 20_000, tokensPerMonth: 120_000_000, maxMemoryPercent: null },
    }),
    app('dyno', 'Dyno', true, false, { requests: 48, promptTokens: 61_200, completionTokens: 18_900, busyMs: 96_000, errors: 1, peakMemoryBytes: 3.6 * GiB, cacheHits: 0, cacheMisses: 0, prefixTokens: 18_400, savedTokens: 0 }, { maxPriority: 'background' }),
    app('nightly-report', 'Nightly report script', false, true, { requests: 391, promptTokens: 702_600, completionTokens: 98_120, busyMs: 840_000, errors: 0, peakMemoryBytes: 3.6 * GiB, cacheHits: 0, cacheMisses: 0, prefixTokens: 240_000, savedTokens: 0 }, {
      extendedApis: false,
      limits: { requestsPerMin: 60, tokensPerDay: 600_000, maxConcurrent: 2, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: 25 },
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
      permissions: { models: null, maxPriority: 'interactive', tools: false, extendedApis: false, cacheResponses: false, limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: 1, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } },
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

/** The memory the demo box can give a model (free RAM plus VRAM, mock only). */
const DEMO_BUDGET = 14.6 * GiB + 4.1 * GiB;

/** KV cache bytes per token of a demo model of `paramsB` billion parameters (about 128 KiB for 8B, mock only). */
function demoKvPerToken(paramsB: number): number {
  return Math.min(512 * 1024, Math.max(16 * 1024, (paramsB / 8) * 128 * 1024));
}

/** How many parallel slots a downloaded-size model would fit at 8,192 tokens each on the demo box (mock only). */
export function demoMaxSlots(size: number): number {
  const base = size * 1.05 + 512 * MiB;
  const per = 8192 * demoKvPerToken((size / GiB) * 1.7);
  if (base + per > DEMO_BUDGET) return 0;
  return Math.min(16, Math.floor((DEMO_BUDGET * 0.8 - base) / per) || 1);
}

/** The slots fit table of a demo model (mock only: the real numbers come from the engine). */
export function demoSlotFit(m: Pick<LocalModel, 'sizeBytes' | 'paramsB' | 'contextMax'>, context: number, slots: number): SlotFit {
  const paramsB = m.paramsB ?? (m.sizeBytes / GiB) * 1.7;
  const kv = demoKvPerToken(paramsB);
  const base = m.sizeBytes * 1.05 + 512 * MiB;
  const per = kv * context;
  const cap = m.contextMax ?? 1_048_576;
  const verdictOf = (n: number): FitVerdict => {
    const need = base + n * per;
    return need <= DEMO_BUDGET * 0.8 ? 'fits' : need <= DEMO_BUDGET ? 'tight' : 'wontFit';
  };
  const maxCtx = (n: number, share: number): number => {
    const room = DEMO_BUDGET * share - base;
    if (room <= 0) return 0;
    return Math.min(cap, Math.floor(room / n / kv / 256) * 256);
  };
  const counts = [...new Set([1, 2, 4, 8, slots])].sort((a, b) => a - b);
  const fitting = (share: number): number => {
    let n = 0;
    while (n < 16 && base + (n + 1) * per <= DEMO_BUDGET * share) n++;
    return n;
  };
  const maxSlots = fitting(1);
  const gb = (n: number): string => `${(n / GiB).toFixed(1)} GB`;
  return {
    context,
    slots,
    verdict: verdictOf(slots),
    baseBytes: base,
    perSlotBytes: per,
    kvExact: m.paramsB !== null,
    maxSlots,
    comfortableSlots: fitting(0.8),
    choices: counts.map((n) => ({ slots: n, verdict: verdictOf(n), needBytes: base + n * per, extraBytes: (n - 1) * per, maxContext: maxCtx(n, 1), comfortableContext: maxCtx(n, 0.8) })),
    message: maxSlots === 0 ? 'Not even one slot of this size fits on this machine. Choose a shorter context or a smaller quantisation.' : `Each slot adds about ${gb(per)}. Up to ${maxSlots} ${maxSlots === 1 ? 'slot fits' : 'slots fit'} at this context.`,
  };
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
    slots: 2,
    slotsCustom: false,
    loadedSlots: null,
    fit: demoFit(p.sizeBytes),
    ...p,
  });
  return [
    m({ id: 'qwen3-8b-instruct-q4_k_m', repo: 'Qwen/Qwen3-8B-Instruct-GGUF', file: 'Qwen3-8B-Instruct-Q4_K_M.gguf', name: 'Qwen3 8B Instruct', quant: 'Q4_K_M', sizeBytes: 5.03 * GiB, paramsB: 8.2, contextMax: 32768, architecture: 'qwen3', capabilities: ['chat', 'tools', 'json'], loaded: true, loadedSlots: 2, lastUsedAt: now - 48_000, addedAt: now - 34 * DAY }),
    m({ id: 'llama-3.2-3b-instruct-q5_k_m', repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF', file: 'Llama-3.2-3B-Instruct-Q5_K_M.gguf', name: 'Llama 3.2 3B Instruct', quant: 'Q5_K_M', sizeBytes: 2.32 * GiB, paramsB: 3.2, contextMax: 131072, architecture: 'llama', capabilities: ['chat', 'tools'], loaded: true, slots: 4, slotsCustom: true, loadedSlots: 4, lastUsedAt: now - 2_000, idleSecs: 300 }),
    m({ id: 'nomic-embed-text-v1.5-f16', repo: 'nomic-ai/nomic-embed-text-v1.5-GGUF', file: 'nomic-embed-text-v1.5.f16.gguf', name: 'nomic-embed-text v1.5', quant: 'F16', sizeBytes: 262 * MiB, paramsB: 0.14, contextMax: 8192, architecture: 'nomic-bert', capabilities: ['embeddings'], loaded: true, pinned: true, lastUsedAt: now - 9_000 }),
    m({ id: 'gemma-3-12b-it-q4_k_m', repo: 'unsloth/gemma-3-12b-it-GGUF', file: 'gemma-3-12b-it-Q4_K_M.gguf', name: 'Gemma 3 12B', quant: 'Q4_K_M', sizeBytes: 6.79 * GiB, paramsB: 12.2, contextMax: 131072, architecture: 'gemma3', capabilities: ['chat', 'vision', 'json'], projector: 'mmproj-F16.gguf', lastUsedAt: now - 2 * DAY, idleSecs: 120 }),
    m({ id: 'mistral-nemo-instruct-2407-q4_k_m', repo: 'bartowski/Mistral-Nemo-Instruct-2407-GGUF', file: 'Mistral-Nemo-Instruct-2407-Q4_K_M.gguf', name: 'Mistral Nemo 12B Instruct', quant: 'Q4_K_M', sizeBytes: 6.96 * GiB, paramsB: 12.2, contextMax: 131072, architecture: 'llama', capabilities: ['chat', 'tools'], lastUsedAt: now - 3 * DAY, addedAt: now - 19 * DAY }),
    m({ id: 'phi-4-mini-instruct-q8_0', repo: 'unsloth/Phi-4-mini-instruct-GGUF', file: 'Phi-4-mini-instruct-Q8_0.gguf', name: 'Phi-4 mini Instruct', quant: 'Q8_0', sizeBytes: 3.82 * GiB, paramsB: 3.8, contextMax: 131072, architecture: 'phi3', capabilities: ['chat', 'tools', 'json'], lastUsedAt: now - 6 * DAY, addedAt: now - 11 * DAY }),
    m({ id: 'bge-reranker-v2-m3-q8_0', repo: 'gpustack/bge-reranker-v2-m3-GGUF', file: 'bge-reranker-v2-m3-Q8_0.gguf', name: 'BGE reranker v2 m3', quant: 'Q8_0', sizeBytes: 606 * MiB, paramsB: 0.57, contextMax: 8192, architecture: 'xlm-roberta', capabilities: ['rerank'], lastUsedAt: now - 5 * HOUR }),
    m({ id: 'qwen2.5-coder-1.5b-q8_0', repo: 'Qwen/Qwen2.5-Coder-1.5B-Instruct-GGUF', file: 'qwen2.5-coder-1.5b-instruct-q8_0.gguf', name: 'Qwen2.5 Coder 1.5B', quant: 'Q8_0', sizeBytes: 1.76 * GiB, paramsB: 1.5, contextMax: 32768, architecture: 'qwen2', capabilities: ['chat'], lastUsedAt: now - 41 * DAY, addedAt: now - 80 * DAY }),
  ];
}

/** Update notices the mock "finds" (invented): a changed file and a better quantisation that fits. */
export function demoUpdateNotices(now: number): UpdateNotice[] {
  return [
    {
      id: 'qwen3-8b-instruct-q4_k_m#newerRevision',
      modelId: 'qwen3-8b-instruct-q4_k_m',
      repo: 'Qwen/Qwen3-8B-Instruct-GGUF',
      kind: 'newerRevision',
      file: 'Qwen3-8B-Instruct-Q4_K_M.gguf',
      quant: 'Q4_K_M',
      sizeBytes: 5.03 * GiB,
      installedFile: 'Qwen3-8B-Instruct-Q4_K_M.gguf',
      installedQuant: 'Q4_K_M',
      summary: 'Qwen3-8B-Instruct-Q4_K_M.gguf changed on Hugging Face since you downloaded it (repository revision 3f9a1c2, now b72e04d).',
      foundAt: now,
      targetSha256: 'b'.repeat(64),
    },
    {
      id: 'llama-3.2-3b-instruct-q5_k_m#betterQuant',
      modelId: 'llama-3.2-3b-instruct-q5_k_m',
      repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF',
      kind: 'betterQuant',
      file: 'Llama-3.2-3B-Instruct-Q6_K.gguf',
      quant: 'Q6_K',
      sizeBytes: 2.64 * GiB,
      installedFile: 'Llama-3.2-3B-Instruct-Q5_K_M.gguf',
      installedQuant: 'Q5_K_M',
      summary: 'Q6_K (2.6 GB) is available and fits this machine; you have Q5_K_M.',
      foundAt: now,
      targetSha256: null,
    },
  ];
}

export function demoLoaded(now: number, idleSecs = 600): LoadedModel[] {
  return [
    { modelId: 'qwen3-8b-instruct-q4_k_m', name: 'Qwen3 8B Instruct', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: 0.9 * GiB, vramBytes: 6.1 * GiB, contextTokens: 16384, slots: 2, pinned: false, activeRequests: 0, loadedAt: now - 52 * MIN, lastUsedAt: now - 48_000, idleReleaseAt: now - 48_000 + idleSecs * 1000, coldStartMs: 3_420, capabilities: ['chat', 'tools', 'json'] },
    { modelId: 'llama-3.2-3b-instruct-q5_k_m', name: 'Llama 3.2 3B Instruct', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'busy', ramBytes: 0.4 * GiB, vramBytes: 2.9 * GiB, contextTokens: 8192, slots: 4, pinned: false, activeRequests: 2, loadedAt: now - 14 * MIN, lastUsedAt: now - 2_000, idleReleaseAt: null, coldStartMs: 1_610, capabilities: ['chat', 'tools'] },
    { modelId: 'nomic-embed-text-v1.5-f16', name: 'nomic-embed-text v1.5', engine: 'llama.cpp', build: 'cuda-13 · b11669', state: 'ready', ramBytes: 0.12 * GiB, vramBytes: 0.33 * GiB, contextTokens: 8192, slots: 1, pinned: true, activeRequests: 0, loadedAt: now - 5 * HOUR, lastUsedAt: now - 9_000, idleReleaseAt: null, coldStartMs: 420, capabilities: ['embeddings'] },
  ];
}

/** The capability floors of the engine's matrix (capabilities.json), as the mock shows them. */
const CAPABILITY_FLOORS: ReadonlyArray<readonly [Capability, string, number]> = [
  ['chat', 'Chat and completion', 0],
  ['json', 'JSON and JSON-schema output', 3000],
  ['tools', 'Tool calling', 4600],
  ['vision', 'Image input (vision projector)', 5600],
  ['embeddings', 'Embeddings', 3000],
  ['rerank', 'Reranking', 4100],
];

/** What a build with this release tag supports, in the engine's own words. */
export function demoBuildCapabilities(tag: string): BuildCapability[] {
  const have = Number(tag.replace(/^b/, '')) || 0;
  return CAPABILITY_FLOORS.map(([capability, label, min]) =>
    have >= min
      ? { capability, label, status: 'supported', minBuild: null, hint: null }
      : { capability, label, status: 'needsBuild', minBuild: min, hint: `Install llama.cpp b${min} or newer in Engines, or unpin the old build.` },
  );
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
        { id: 'cpu', tag: 'b4300', label: 'CPU (AVX2)', sizeBytes: 24 * MiB, selfTest: 'notRun', selfTestMessage: null, devices: ['CPU'], active: false, outdated: true },
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
      rollbackTo: 'vulkan',
    },
    {
      kind: 'pack',
      name: 'ik_llama.cpp (pack)',
      description: 'A community fork with faster CPU and hybrid inference, added as a declarative engine pack (ADR 0011).',
      availability: { state: 'notInstalled', reason: 'Install a build to serve models with this engine.' },
      buildChoice: 'auto',
      builds: [],
      catalog: [{ id: 'ik-llama-cpu', label: 'CPU (AVX2)', tag: 'v0.9.2', downloadBytes: 29 * MiB, suggested: true, installed: false }],
      processes: [],
      rollbackTo: null,
    },
    { kind: 'whisper.cpp', name: 'whisper.cpp', description: 'Speech to text (OpenAI-compatible /v1/audio/transcriptions).', availability: { state: 'notInstalled', reason: 'Planned for a later version: transcription is not wired to the router yet.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
    { kind: 'mlx', name: 'MLX', description: "Apple's array framework for Apple silicon.", availability: { state: 'notAvailable', reason: 'MLX runs on Apple silicon only; this is a Windows PC.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
    { kind: 'vllm', name: 'vLLM', description: 'High-throughput serving with paged attention.', availability: { state: 'notAvailable', reason: 'vLLM needs Linux with a CUDA GPU; Windows is not supported.' }, buildChoice: 'auto', builds: [], catalog: [], processes: [], rollbackTo: null },
  ];
}

export function demoSettings(): Settings {
  return {
    general: { theme: 'system', keepServingWhenClosed: true },
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
    cache: {
      prefixAffinity: true,
      promptCacheMib: null,
      embeddings: true,
      embeddingsStorage: 'disk',
      embeddingsMaxMib: 512,
      embeddingsTtlSecs: 90 * 86_400,
      responsesStorage: 'disk',
      responsesMaxMib: 256,
      responsesTtlSecs: 7 * 86_400,
      keepWarmDays: 30,
    },
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
    return { file, quant, sizeBytes: size, parts, sha256: null, fit: demoFit(size), maxSlots: demoMaxSlots(size), downloaded: local.some((l) => l.repo === repo && l.quant === quant) };
  });
  const vision = /gemma-3|vision|vl/i.test(repo);
  return {
    repo,
    variants,
    projectors: vision ? [{ file: 'mmproj-F16.gguf', quant: 'F16', sizeBytes: 854 * MiB, parts: 1, sha256: null, fit: demoFit(854 * MiB), maxSlots: 0, downloaded: false }] : [],
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
const APP_KEYS = ['debrief', 'spotter', 'nightly-report', 'pitcrew', 'dyno'];

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
  events.push({ id, at: start + span * 0.62, kind: 'engineRestarted', severity: 'warn', message: 'llama.cpp (Llama 3.2 3B) stopped responding and was restarted', modelId: ids[1]!, appId: null, reason: null });
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
    e(14, 'modelLoaded', 'info', 'Loaded Llama 3.2 3B Instruct for Nightly report script (1.6 s cold start)', { modelId: 'llama-3.2-3b-instruct-q5_k_m', appId: 'nightly-report' }),
    e(22, 'modelUnloaded', 'warn', 'Released Gemma 3 12B: available RAM fell below 15 %', { modelId: 'gemma-3-12b-it-q4_k_m', reason: 'pressure' }),
    e(31, 'requestFailed', 'error', 'Spotter: 429 rate limit (600 requests/min)', { appId: 'spotter' }),
    e(48, 'download', 'info', 'Downloaded bge-reranker-v2-m3 Q8_0 (606 MB, verified)'),
    e(52, 'modelLoaded', 'info', 'Loaded Qwen3 8B Instruct for Debrief (3.4 s cold start)', { modelId: 'qwen3-8b-instruct-q4_k_m', appId: 'debrief' }),
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
    if (appId === 'nightly-report') [api, endpoint] = rnd() > 0.5 ? ENDPOINTS[3]! : ENDPOINTS[5]!;
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
      alias: null,
      priority: appId === 'spotter' ? 'batch' : appId === 'pitcrew' || appId === 'dyno' ? 'background' : 'interactive',
      status,
      stream: !embedding && rnd() > 0.3,
      queuedMs,
      ttftMs: ttft,
      totalMs: Math.round(queuedMs + (ttft ?? 40) + completion * (18 + rnd() * 10) + (embedding ? 20 + rnd() * 80 : 0)),
      coldStartMs: cold,
      promptTokens: Math.round(embedding ? 64 + rnd() * 400 : 200 + rnd() * 3800),
      completionTokens: completion,
      cache: null,
      cachedTokens: 0,
      error: status === 429 ? 'Rate limit: 600 requests/min' : status === 500 ? 'Engine stopped responding' : status === 400 ? 'Unknown model "gpt-4o"' : null,
      hasContent: i % 3 !== 2,
    });
    at -= Math.round(2_000 + rnd() * 25_000);
  }
  // Cache outcomes: Spotter's embeddings and Debrief's opted-in answers are cache-eligible; chat prompts reuse prefixes.
  out.forEach((r, i) => {
    if (r.status !== 200) return;
    if (r.appId === 'spotter' && r.endpoint.includes('embed')) {
      r.cache = i % 3 === 0 ? 'miss' : 'hit';
      if (r.cache === 'hit') r.totalMs = Math.min(r.totalMs, 6 + (i % 9));
    } else if (r.appId === 'debrief' && !r.endpoint.includes('embed')) {
      r.cache = i % 5 === 0 ? 'hit' : i % 5 === 1 ? 'miss' : i % 7 === 0 ? 'bypass' : null;
      if (r.cache === 'hit') {
        r.totalMs = Math.min(r.totalMs, 12 + (i % 11));
        r.ttftMs = 4;
        r.completionTokens = Math.max(r.completionTokens, 40);
      }
    }
    if (r.promptTokens > 1000 && r.cache !== 'hit' && !r.endpoint.includes('embed') && i % 2 === 0) r.cachedTokens = Math.round(r.promptTokens * 0.8);
  });
  // Some apps ask for a preset by name; `modelId` is what it resolved to.
  out.forEach((r, i) => {
    if (r.status === 400 || r.endpoint.includes('embed') || r.endpoint.includes('rerank')) return;
    if (r.appId === 'debrief' && i % 3 === 0) Object.assign(r, { alias: 'debrief-explain', modelId: 'qwen3-8b-instruct-q4_k_m', chosenBy: 'explicit' });
    if (r.appId === 'pitcrew' && i % 2 === 0) Object.assign(r, { alias: 'pitcrew-triage', chosenBy: 'capabilities' });
  });
  return out;
}

/** Presets an engineer might have set up. `legacy-explain` points at a model that was removed. */
export function demoPresets(): ModelPreset[] {
  const base = { context: null, temperature: null, topP: null, maxTokens: null, systemPrompt: null, engine: null, cacheResponses: false, keepWarm: false };
  return [
    { ...base, alias: 'debrief-explain', target: 'qwen3-8b-instruct-q4_k_m', temperature: 0.3, maxTokens: 800, cacheResponses: true, keepWarm: true, systemPrompt: 'Explain the log excerpt to an engineer in plain language. Be concise and name the likely cause first.' },
    { ...base, alias: 'pitcrew-triage', target: 'auto:chat,tools,fast', context: 16384, temperature: 0.1, keepWarm: true },
    { ...base, alias: 'spotter-summarise', target: 'llama-3.2-3b-instruct-q5_k_m', temperature: 0.2, topP: 0.9, maxTokens: 300, systemPrompt: 'Summarise the text in three short bullet points.' },
    { ...base, alias: 'legacy-explain', target: 'mistral-7b-instruct-v0.3-q4_k_m' },
  ];
}

export function demoBenchmarks(now: number): BenchmarkResult[] {
  return [
    { modelId: 'qwen3-8b-instruct-q4_k_m', build: 'cuda-13 · b11669', at: now - 2 * DAY, promptTps: 2140, genTps: 61.8, ttftMs: 212, contextTokens: 4096 },
    { modelId: 'llama-3.2-3b-instruct-q5_k_m', build: 'cuda-13 · b11669', at: now - 2 * DAY, promptTps: 4380, genTps: 118.4, ttftMs: 96, contextTokens: 4096 },
    { modelId: 'gemma-3-12b-it-q4_k_m', build: 'cuda-13 · b11669', at: now - 6 * DAY, promptTps: 1210, genTps: 38.9, ttftMs: 384, contextTokens: 4096 },
    { modelId: 'qwen2.5-coder-1.5b-q8_0', build: 'vulkan · b11669', at: now - 12 * DAY, promptTps: 5120, genTps: 141.2, ttftMs: 71, contextTokens: 4096 },
    { modelId: 'mistral-nemo-instruct-2407-q4_k_m', build: 'cuda-13 · b11669', at: now - 4 * DAY, promptTps: 1380, genTps: 44.6, ttftMs: 342, contextTokens: 4096 },
    { modelId: 'phi-4-mini-instruct-q8_0', build: 'cuda-13 · b11669', at: now - 5 * DAY, promptTps: 3210, genTps: 83.7, ttftMs: 128, contextTokens: 4096 },
    { modelId: 'qwen3-8b-instruct-q4_k_m', build: 'vulkan · b11669', at: now - 3 * DAY, promptTps: 1490, genTps: 52.3, ttftMs: 268, contextTokens: 4096 },
  ];
}

export const DEMO_REPLY =
  'Pitwall keeps models hot only while they are useful. When a model has been idle for the configured time it first releases the context (the KV cache), and later the weights, so your games and IDE get the memory back. Under memory pressure it releases the least recently used model at once, and pinned models stay loaded until you unpin them.';

/** The demo reply as streamed words: the small model says less, always in whole sentences. */
export function demoReplyWords(modelId: string): string[] {
  const sentences = DEMO_REPLY.match(/[^.]+\.\s*/g) ?? [DEMO_REPLY];
  const text = modelId.includes('llama') ? sentences.slice(0, 2).join('') : DEMO_REPLY;
  return text.trim().split(/(?<= )/);
}

/** How many of `words` fit in `maxTokens`: the longest run of whole sentences, or the plain cut when not even one sentence fits. */
export function wordsThatFit(words: string[], maxTokens: number): number {
  const limit = Math.max(1, Math.min(words.length, Math.floor(maxTokens / 1.3)));
  if (limit >= words.length) return words.length;
  for (let n = limit; n > 0; n--) if (/[.!?]\s*$/.test(words[n - 1]!)) return n;
  return limit;
}

// ---- Packs -------------------------------------------------------------------------------------------------------

interface DemoCatalogModel {
  id: string;
  name: string;
  repo: string;
  base: string;
  quant: string;
  sizeGiB: number;
  tags: string[];
}

const DEMO_CATALOG: DemoCatalogModel[] = [
  { id: 'qwen2.5-coder-7b', name: 'Qwen2.5 Coder 7B Instruct', repo: 'bartowski/Qwen2.5-Coder-7B-Instruct-GGUF', base: 'Qwen2.5-Coder-7B-Instruct', quant: 'Q4_K_M', sizeGiB: 4.4, tags: ['code', 'chat'] },
  { id: 'qwen3-1.7b', name: 'Qwen3 1.7B', repo: 'unsloth/Qwen3-1.7B-GGUF', base: 'Qwen3-1.7B', quant: 'Q8_0', sizeGiB: 1.8, tags: ['small', 'chat'] },
  { id: 'llama-3.2-3b', name: 'Llama 3.2 3B Instruct', repo: 'bartowski/Llama-3.2-3B-Instruct-GGUF', base: 'Llama-3.2-3B-Instruct', quant: 'Q5_K_M', sizeGiB: 2.3, tags: ['small', 'chat'] },
  { id: 'gemma-3-12b', name: 'Gemma 3 12B Instruct', repo: 'unsloth/gemma-3-12b-it-GGUF', base: 'gemma-3-12b-it', quant: 'Q4_K_M', sizeGiB: 6.9, tags: ['chat', 'vision'] },
  { id: 'qwen3-32b', name: 'Qwen3 32B', repo: 'unsloth/Qwen3-32B-GGUF', base: 'Qwen3-32B', quant: 'Q4_K_M', sizeGiB: 19.8, tags: ['chat', 'reasoning'] },
  { id: 'qwen3-coder-30b', name: 'Qwen3 Coder 30B A3B', repo: 'unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF', base: 'Qwen3-Coder-30B-A3B-Instruct', quant: 'Q4_K_M', sizeGiB: 18.6, tags: ['code'] },
  { id: 'nomic-embed', name: 'Nomic Embed Text v1.5', repo: 'nomic-ai/nomic-embed-text-v1.5-GGUF', base: 'nomic-embed-text-v1.5', quant: 'F16', sizeGiB: 0.26, tags: ['small', 'embedding'] },
];

/** The demo packs (mock only): recommendations sorted fits first, optionally for one tag; one pack with a problem. */
export function demoPacks(tag: string | null): PacksInfo {
  const rank = { fits: 0, tight: 1, unknown: 2, wontFit: 3 } as const;
  const recommendations = DEMO_CATALOG.filter((m) => !tag || m.tags.includes(tag))
    .map((m): Recommendation => {
      const sizeBytes = Math.round(m.sizeGiB * GiB);
      return { id: m.id, name: m.name, repo: m.repo, file: `${m.base}-${m.quant}.gguf`, quant: m.quant, sizeBytes, tags: m.tags, fit: demoFit(sizeBytes), contextTokens: 8192 };
    })
    .sort((a, b) => rank[a.fit.verdict] - rank[b.fit.verdict] || a.sizeBytes - b.sizeBytes);
  return {
    engines: [{ id: 'ik-llama-cpu', name: 'ik_llama.cpp (pack)', version: '0.9.2' }],
    problems: [{ file: 'packs/old-vllm.toml', message: 'skipped: unknown key `launch.cmd`; engine packs need `[launch] args`.' }],
    warnings: [{ file: 'packs/ik-llama.toml', message: 'unknown key `notes` ignored.' }],
    recommendations,
  };
}

// ---- Usage over time ---------------------------------------------------------------------------------------------

const USAGE_STEP: Record<UsageRange, { step: number; buckets: number }> = {
  '24h': { step: HOUR, buckets: 24 },
  '7d': { step: HOUR, buckets: 168 },
  '30d': { step: DAY, buckets: 30 },
};

/** Per app: relative request volume, prompt and completion tokens per request, error rate and the model footprint. */
const USAGE_PROFILE: Record<string, { weight: number; prompt: number; completion: number; errors: number; memory: number; cacheShare: number; hitRate: number; prefix: number }> = {
  debrief: { weight: 1.0, prompt: 1500, completion: 220, errors: 0.003, memory: 7.4 * GiB, cacheShare: 0.4, hitRate: 0.35, prefix: 700 },
  pitcrew: { weight: 0.16, prompt: 700, completion: 150, errors: 0.01, memory: 3.6 * GiB, cacheShare: 0, hitRate: 0, prefix: 300 },
  spotter: { weight: 2.6, prompt: 260, completion: 4, errors: 0.004, memory: 0.5 * GiB, cacheShare: 1, hitRate: 0.64, prefix: 0 },
  dyno: { weight: 0.04, prompt: 1200, completion: 390, errors: 0.02, memory: 3.6 * GiB, cacheShare: 0, hitRate: 0, prefix: 400 },
  'nightly-report': { weight: 0.3, prompt: 1800, completion: 250, errors: 0, memory: 3.6 * GiB, cacheShare: 0, hitRate: 0, prefix: 500 },
};

/** Usage over a range for the demo apps (mock only): daytime-heavy hours, quiet nights, complete buckets. */
export function demoUsageSeries(range: UsageRange, now: number, appId: string | null, known: string[] = Object.keys(USAGE_PROFILE), seed = 11): UsageSeries {
  const { step, buckets } = USAGE_STEP[range];
  const current = now - (now % step);
  const starts = Array.from({ length: buckets }, (_, i) => current - (buckets - 1 - i) * step);
  const ids = appId ? [appId] : known;
  const empty = (): UsageCounters => ({ requests: 0, promptTokens: 0, completionTokens: 0, busyMs: 0, errors: 0, peakMemoryBytes: 0, cacheHits: 0, cacheMisses: 0, prefixTokens: 0, savedTokens: 0 });
  const merge = (a: UsageCounters, b: UsageCounters) => {
    a.requests += b.requests;
    a.promptTokens += b.promptTokens;
    a.completionTokens += b.completionTokens;
    a.busyMs += b.busyMs;
    a.errors += b.errors;
    a.cacheHits += b.cacheHits;
    a.cacheMisses += b.cacheMisses;
    a.prefixTokens += b.prefixTokens;
    a.savedTokens += b.savedTokens;
    a.peakMemoryBytes = Math.max(a.peakMemoryBytes, b.peakMemoryBytes);
  };
  const apps: AppUsageSeries[] = ids.map((id, k) => {
    const profile = USAGE_PROFILE[id] ?? { weight: 0, prompt: 0, completion: 0, errors: 0, memory: 0, cacheShare: 0, hitRate: 0, prefix: 0 };
    const rnd = prng(seed + k * 97 + range.length);
    const totals = empty();
    const points = starts.map((at) => {
      const h = new Date(at).getHours();
      const day = step === DAY ? 1 : 0.15 + 0.85 * Math.max(0, Math.sin(((h - 7) / 14) * Math.PI));
      const requests = Math.round(profile.weight * day * (step === DAY ? 900 : 60) * (0.6 + rnd() * 0.8));
      const eligible = Math.round(requests * profile.cacheShare * (0.8 + rnd() * 0.4));
      const hits = Math.round(eligible * profile.hitRate * (0.8 + rnd() * 0.4));
      const prefixTokens = Math.round(requests * profile.prefix * (0.8 + rnd() * 0.4));
      const cache = { cacheHits: hits, cacheMisses: eligible - hits, prefixTokens, savedTokens: Math.round(hits * profile.prompt * 0.9) };
      const c: UsageCounters = {
        requests,
        promptTokens: Math.round(requests * profile.prompt * (0.8 + rnd() * 0.4)),
        completionTokens: Math.round(requests * profile.completion * (0.8 + rnd() * 0.4)),
        busyMs: Math.round(requests * (300 + profile.completion * 22)),
        errors: Math.round(requests * profile.errors * (rnd() * 2)),
        peakMemoryBytes: requests ? profile.memory : 0,
        ...cache,
      };
      merge(totals, c);
      return c;
    });
    return { appId: id, points, totals };
  });
  apps.sort((a, b) => b.totals.promptTokens + b.totals.completionTokens - (a.totals.promptTokens + a.totals.completionTokens));
  const total = starts.map((_, i) => {
    const c = empty();
    for (const a of apps) merge(c, a.points[i]!);
    return c;
  });
  const totals = empty();
  for (const c of total) merge(totals, c);
  return { range, stepMs: step, starts, total, totals, apps };
}

/** Where each configured limit stands (mock only: the real statuses come from the server). */
export function demoLimitStatuses(limits: AppLimits, today: UsageCounters, month: UsageCounters, machineBytes: number): LimitStatus[] {
  const status = (kind: LimitStatus['kind'], used: number, limit: number): LimitStatus => {
    const percent = Math.min(999, Math.floor((used * 100) / limit));
    const state = percent >= 100 ? 'exceeded' : percent >= 80 ? 'warning' : 'ok';
    const message =
      state === 'warning' ? `${percent}% of the limit (${limit}) is used; requests are refused at 100%.` : state === 'exceeded' ? `The limit (${limit}) is reached; requests are refused until it resets.` : null;
    return { kind, used, limit, percent, state, message };
  };
  const out: LimitStatus[] = [];
  if (limits.requestsPerMin !== null) out.push(status('requestsPerMin', Math.min(limits.requestsPerMin - 1, 12), limits.requestsPerMin));
  if (limits.requestsPerDay !== null) out.push(status('requestsPerDay', today.requests, limits.requestsPerDay));
  if (limits.tokensPerDay !== null) out.push(status('tokensPerDay', today.promptTokens + today.completionTokens, limits.tokensPerDay));
  if (limits.tokensPerMonth !== null) out.push(status('tokensPerMonth', month.promptTokens + month.completionTokens, limits.tokensPerMonth));
  if (limits.maxMemoryPercent !== null) out.push(status('memoryShare', today.peakMemoryBytes, Math.floor((machineBytes * limits.maxMemoryPercent) / 100)));
  return out;
}

/** One app in detail for the mock: this month from the 30-day series, limits from today's counters. */
export function demoAppDetail(app: AppInfo, requests: RequestRecord[], now: number): AppDetail {
  const month = demoUsageSeries('30d', now, app.id).totals;
  return {
    app,
    month,
    limits: demoLimitStatuses(app.permissions.limits, app.today, month, RAM_TOTAL),
    recent: requests.filter((r) => r.appId === app.id).slice(0, 20),
  };
}
