import { SU_ACCENTS, type SuAccent } from '@decryp7/paddock-ui';
import type { AiState } from '@decryp7/paddock-ui/shell/pitwall';

/*
 * The browser demo's URL switches (review and screenshots only; the desktop app ignores them): `?accent=pacenotes` shows a
 * real app accent instead of the neutral graphite, `?pitwall=ready` lets the assistant answer (its memory backend).
 */

const param = (name: string): string | null => {
  try {
    return new URLSearchParams(globalThis.location?.search ?? '').get(name);
  } catch {
    return null;
  }
};

/** The accent of the window: `?accent=` when it names one, else graphite (the demos' neutral accent). */
export function demoAccent(): SuAccent {
  const a = param('accent');
  return (SU_ACCENTS as readonly string[]).includes(a ?? '') ? (a as SuAccent) : 'graphite';
}

/** Pitwall's state in the browser: ready with `?pitwall=ready`, else installed but not running. */
export function demoPitwallState(): AiState {
  return param('pitwall') === 'ready' ? { kind: 'ready', model: 'qwen3-8b-q4km', vision: false, contextTokens: 8192 } : { kind: 'notRunning' };
}
