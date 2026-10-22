/** Display-only parsing of a runner log line in logrus' text format (`time="…" level=info msg="…"`). */

export type LogLevel = 'trace' | 'debug' | 'info' | 'warn' | 'error' | 'fatal' | 'panic';

export interface ParsedLine {
  time: string | null;
  level: LogLevel | null;
  msg: string;
}

const LEVELS = new Set<LogLevel>(['trace', 'debug', 'info', 'warn', 'error', 'fatal', 'panic']);
const TIME = /time="([^"]*)"/;
const LEVEL = /level=(\w+)/;
const MSG = /msg="((?:[^"\\]|\\.)*)"/;

/** Splits a line into time, level and message for colouring; anything else is shown as it is. */
export function parseLogLine(text: string): ParsedLine {
  const level = LEVEL.exec(text)?.[1]?.toLowerCase();
  const msg = MSG.exec(text)?.[1];
  if (!level || msg === undefined) return { time: null, level: null, msg: text };
  const time = TIME.exec(text)?.[1] ?? null;
  return {
    time: time ? shortTime(time) : null,
    level: level === 'warning' ? 'warn' : LEVELS.has(level as LogLevel) ? (level as LogLevel) : null,
    msg: msg.replace(/\\"/g, '"').replace(/\\\\/g, '\\'),
  };
}

/** `2026-10-22T10:04:05+02:00` → `10:04:05` (the date is in the full line). */
function shortTime(iso: string): string {
  const m = /T(\d{2}:\d{2}:\d{2})/.exec(iso);
  return m?.[1] ?? iso;
}

/** Splits text around case-insensitive matches of `query`, for highlighting with text bindings (never HTML). */
export function highlight(text: string, query: string): { text: string; match: boolean }[] {
  const q = query.trim().toLowerCase();
  if (!q) return [{ text, match: false }];
  const out: { text: string; match: boolean }[] = [];
  const lower = text.toLowerCase();
  let at = 0;
  for (let i = lower.indexOf(q); i >= 0; i = lower.indexOf(q, at)) {
    if (i > at) out.push({ text: text.slice(at, i), match: false });
    out.push({ text: text.slice(i, i + q.length), match: true });
    at = i + q.length;
  }
  if (at < text.length) out.push({ text: text.slice(at), match: false });
  return out;
}

/** The colour role of a level. */
export function levelTone(level: LogLevel | null): 'danger' | 'warning' | 'neutral' | 'info' {
  if (level === 'error' || level === 'fatal' || level === 'panic') return 'danger';
  if (level === 'warn') return 'warning';
  if (level === 'debug' || level === 'trace') return 'neutral';
  return 'info';
}
