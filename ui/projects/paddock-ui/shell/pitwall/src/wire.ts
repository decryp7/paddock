import type { AiSnapshot, AiState, Caps, NextStep, PairingView } from '../generated/pitwall';

const KINDS = new Set<AiState['kind']>(['disabled', 'notInstalled', 'notRunning', 'incompatible', 'needsPairing', 'paused', 'noSuitableModel', 'unavailable', 'ready']);

const isRecord = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v);
const text = (v: unknown, max: number): string | null => (typeof v === 'string' && v.length <= max ? v : null);

function parseCaps(v: unknown): Caps | null {
  if (!isRecord(v)) return null;
  const flag = (k: string): boolean | null => (typeof v[k] === 'boolean' ? (v[k] as boolean) : null);
  const [chat, vision, tools, json, embeddings, fast] = ['chat', 'vision', 'tools', 'json', 'embeddings', 'fast'].map(flag);
  const ctx = v['contextTokens'];
  if ([chat, vision, tools, json, embeddings, fast].includes(null) || !(ctx === null || typeof ctx === 'number')) return null;
  return { chat: chat!, vision: vision!, tools: tools!, json: json!, embeddings: embeddings!, fast: fast!, contextTokens: ctx as number | null };
}

function parseState(v: unknown): AiState | null {
  if (!isRecord(v) || typeof v['kind'] !== 'string' || !KINDS.has(v['kind'] as AiState['kind'])) return null;
  switch (v['kind'] as AiState['kind']) {
    case 'incompatible': {
      const message = text(v['message'], 600);
      return message !== null && typeof v['updateApp'] === 'boolean' ? { kind: 'incompatible', message, updateApp: v['updateApp'] } : null;
    }
    case 'paused':
      return typeof v['stopped'] === 'boolean' ? { kind: 'paused', stopped: v['stopped'] } : null;
    case 'noSuitableModel': {
      const needs = parseCaps(v['needs']);
      return needs ? { kind: 'noSuitableModel', needs } : null;
    }
    case 'unavailable': {
      const message = text(v['message'], 600);
      return message !== null ? { kind: 'unavailable', message } : null;
    }
    case 'ready': {
      const model = text(v['model'], 200);
      return model !== null && typeof v['vision'] === 'boolean' && typeof v['contextTokens'] === 'number'
        ? { kind: 'ready', model, vision: v['vision'], contextTokens: v['contextTokens'] }
        : null;
    }
    default:
      return { kind: v['kind'] } as AiState;
  }
}

const STEPS = new Set<NextStep>(['startPitwall', 'installPitwall', 'pair', 'installModel', 'wait', 'updateApp', 'updatePitwall', 'changePermissions', 'none']);

function parseActions(v: unknown): NextStep[] | null {
  if (!Array.isArray(v) || v.length > 8 || !v.every((a) => STEPS.has(a as NextStep))) return null;
  return v as NextStep[];
}

function parsePairing(v: unknown): PairingView | null {
  if (!isRecord(v)) return null;
  const code = text(v['code'], 16);
  return code && typeof v['expiresAtMs'] === 'number' ? { code, expiresAtMs: v['expiresAtMs'] } : null;
}

/** Parses the engine's snapshot at the edge (rule: parse, don't validate); anything else is `null`. Unknown fields are ignored. */
export function parseSnapshot(payload: unknown): AiSnapshot | null {
  if (!isRecord(payload)) return null;
  const state = parseState(payload['state']);
  const app = text(payload['app'], 64);
  const pairing = payload['pairing'] === null || payload['pairing'] === undefined ? null : parsePairing(payload['pairing']);
  if (!state || !app || (payload['pairing'] != null && !pairing)) return null;
  const message = text(payload['message'], 800);
  const actions = parseActions(payload['actions']);
  if (message === null || !actions) return null;
  if (typeof payload['builtInAvailable'] !== 'boolean' || typeof payload['builtInForSession'] !== 'boolean') return null;
  return { state, app, message, actions, pairing, builtInAvailable: payload['builtInAvailable'], builtInForSession: payload['builtInForSession'] };
}

/** The six-digit code in two groups of three ("482 913") as Pitwall shows it, so the user can compare. */
export function groupCode(code: string): string {
  return code.length === 6 ? `${code.slice(0, 3)} ${code.slice(3)}` : code;
}

/** What a refused or failed command says: the engine's message when it sent one. */
export function pitwallMessage(e: unknown, fallback: string): string {
  if (typeof e === 'string' && e) return e;
  if (e instanceof Error && e.message) return e.message;
  if (isRecord(e) && typeof e['message'] === 'string' && e['message']) return e['message'];
  return fallback;
}
