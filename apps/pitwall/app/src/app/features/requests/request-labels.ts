import type { ApiFamily, ChosenBy } from '../../core/api.types';
import { compact } from '../../shared/format';

export const API_LABEL: Record<ApiFamily, string> = {
  openai: 'OpenAI',
  anthropic: 'Anthropic',
  ollama: 'Ollama',
  extended: 'Pitwall /x',
};

export const CHOSEN_LABEL: Record<ChosenBy, string> = {
  explicit: 'Named',
  capabilities: 'Capabilities',
  default: 'Default',
};

const HTTP_TEXT: Record<number, string> = {
  400: 'Bad request',
  401: 'Unauthorised',
  403: 'Forbidden',
  404: 'Not found',
  408: 'Timed out',
  413: 'Too large',
  422: 'Unprocessable',
  429: 'Rate limited',
  500: 'Server error',
  502: 'Bad gateway',
  503: 'Unavailable',
  504: 'Timed out',
};

/** An error status as the log shows it: `429 Rate limited`; an unlisted code stays a bare number. */
export function statusText(status: number): string {
  const text = HTTP_TEXT[status];
  return text ? `${status} ${text}` : String(status);
}

/** Total time in seconds for a column headed "Total s" (`2.9`, `0.2`, `13`); `—` when absent. */
export function secondsCell(ms: number | null): string {
  if (ms === null) return '—';
  const s = ms / 1000;
  return s >= 100 ? String(Math.round(s)) : s.toFixed(s >= 10 ? 0 : 1);
}

/** In and out tokens as one cell: `212 / 1.4k`. */
export function tokensCell(promptTokens: number, completionTokens: number): string {
  return `${compact(promptTokens)} / ${compact(completionTokens)}`;
}
