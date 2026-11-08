import type { AuditRow, KeyInfo, McpError, McpErrorCode, McpEvent, McpSettings, McpView, Outcome, ToolInfo } from '../generated/mcp';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped here, and what passes is
// rebuilt from known fields only.

const OUTCOMES: ReadonlySet<string> = new Set<Outcome>(['ok', 'tool-error', 'refused', 'timed-out', 'panicked', 'cancelled']);
const CODES: ReadonlySet<string> = new Set<McpErrorCode>(['invalid', 'storeUnavailable', 'failed']);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function strings(v: unknown): string[] | null {
  return Array.isArray(v) && v.every((s) => typeof s === 'string') ? (v as string[]) : null;
}

function count(v: unknown): v is number {
  return typeof v === 'number' && Number.isFinite(v) && v >= 0;
}

/** `McpSettings`, or null when the payload is not one. */
export function parseSettings(v: unknown): McpSettings | null {
  if (!isObject(v)) return null;
  const disabledTools = strings(v['disabledTools']);
  const allowedActions = strings(v['allowedActions']);
  if (typeof v['enabled'] !== 'boolean' || typeof v['resources'] !== 'boolean' || !disabledTools || !allowedActions) return null;
  return { enabled: v['enabled'], resources: v['resources'], disabledTools, allowedActions };
}

function parseTool(v: unknown): ToolInfo | null {
  if (!isObject(v)) return null;
  const title = v['title'] ?? null;
  if (typeof v['name'] !== 'string' || typeof v['description'] !== 'string' || (title !== null && typeof title !== 'string')) return null;
  if (typeof v['action'] !== 'boolean' || typeof v['exposed'] !== 'boolean') return null;
  return { name: v['name'], title, description: v['description'], action: v['action'], exposed: v['exposed'] };
}

function parseKey(v: unknown): KeyInfo | null {
  if (!isObject(v) || !count(v['id']) || typeof v['client'] !== 'string' || typeof v['createdMs'] !== 'number' || typeof v['revoked'] !== 'boolean') return null;
  return { id: v['id'], client: v['client'], createdMs: v['createdMs'], revoked: v['revoked'] };
}

/** The listener state; anything unknown or malformed reads as `unavailable` (the page then shows no connection section). */
function parseListener(v: unknown): McpView['listener'] {
  if (!isObject(v)) return { kind: 'unavailable' };
  switch (v['kind']) {
    case 'off':
    case 'noKey':
      return { kind: v['kind'] };
    case 'listening':
      return count(v['port']) && typeof v['discoveryFile'] === 'string' ? { kind: 'listening', port: v['port'], discoveryFile: v['discoveryFile'] } : { kind: 'unavailable' };
    case 'failed':
      return typeof v['message'] === 'string' ? { kind: 'failed', message: v['message'] } : { kind: 'unavailable' };
    default:
      return { kind: 'unavailable' };
  }
}

/** A `McpView` from `mcp_get` / `mcp_save` / `mcp_revoke_key`, or null when the payload is not one. */
export function parseMcpView(v: unknown): McpView | null {
  if (!isObject(v) || !Array.isArray(v['tools']) || !Array.isArray(v['keys']) || !count(v['maxKeys'])) return null;
  const settings = parseSettings(v['settings']);
  if (!settings) return null;
  const tools = (v['tools'] as unknown[]).map(parseTool);
  const keys = (v['keys'] as unknown[]).map(parseKey);
  if (tools.includes(null) || keys.includes(null)) return null;
  return { settings, tools: tools as ToolInfo[], keys: keys as KeyInfo[], maxKeys: v['maxKeys'], listener: parseListener(v['listener']), keysProblem: typeof v['keysProblem'] === 'string' && v['keysProblem'] !== '' ? v['keysProblem'] : null };
}

/** The key `mcp_issue_key` answered with, or null. It is shown once and kept nowhere. */
export function parseIssuedKey(v: unknown): { id: number; client: string; key: string } | null {
  if (!isObject(v) || !count(v['id']) || typeof v['client'] !== 'string' || typeof v['key'] !== 'string' || v['key'] === '') return null;
  return { id: v['id'], client: v['client'], key: v['key'] };
}

function parseRow(v: unknown): AuditRow | null {
  if (!isObject(v)) return null;
  const { atMs, client, tool, resultBytes, durationMs, outcome } = v;
  if (typeof atMs !== 'number' || typeof client !== 'string' || typeof tool !== 'string' || !count(resultBytes) || !count(durationMs)) return null;
  if (typeof outcome !== 'string' || !OUTCOMES.has(outcome)) return null;
  return { atMs, client, tool, arguments: v['arguments'] ?? null, resultBytes, durationMs, outcome: outcome as Outcome };
}

/** The rows of `mcp_audit`, or null when the payload is not a list of rows. */
export function parseAuditRows(v: unknown): AuditRow[] | null {
  if (!Array.isArray(v)) return null;
  const rows = v.map(parseRow);
  return rows.includes(null) ? null : (rows as AuditRow[]);
}

/** The engine's refusal (`McpError`) if `e` is one. */
export function asMcpError(e: unknown): McpError | null {
  if (!isObject(e) || typeof e['code'] !== 'string' || !CODES.has(e['code']) || typeof e['message'] !== 'string') return null;
  return { code: e['code'] as McpErrorCode, message: e['message'], retryable: e['retryable'] === true };
}

/** A `McpEvent` from `paddock://mcp`, or null. */
export function parseMcpEvent(v: unknown): McpEvent | null {
  if (!isObject(v) || v['kind'] !== 'audit' || !count(v['count'])) return null;
  return { kind: 'audit', count: v['count'] };
}

/** A message for the user from anything a command rejected with: the engine's own, or a plain message. */
export function mcpMessage(e: unknown, fallback: string): string {
  const known = asMcpError(e);
  if (known) return known.message;
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
