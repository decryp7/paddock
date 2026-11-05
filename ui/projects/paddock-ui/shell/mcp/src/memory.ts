import { MAX_KEYS, type AuditRow, type McpError, type McpEvent, type McpSettings, type McpView, type ToolInfo } from '../generated/mcp';
import { McpBackend, type IssuedMcpKey, type Unlisten } from './backend';

/** A tool the fake engine registers. */
export interface MemoryTool {
  readonly name: string;
  readonly title?: string;
  readonly description: string;
  /** An action tool changes something and needs its own permission. */
  readonly action?: boolean;
}

export interface MemoryMcpOptions {
  /** The tools the app registered (default: three read-only tools and one action). */
  readonly tools?: readonly MemoryTool[];
  /** The settings at the start (default: switched off). */
  readonly settings?: McpSettings;
  /** The reason the saved keys are not in use, as the engine words it (default none). */
  readonly keysProblem?: string;
  /** `get` rejects this many times before it answers (to show the inline error and Retry). */
  readonly getFailures?: number;
  /** `save` rejects with this error. */
  readonly saveError?: McpError;
  /** `issueKey` rejects with this error. */
  readonly issueError?: McpError;
  /** `revokeKey` rejects with this error. */
  readonly revokeError?: McpError;
  /** `audit` rejects this many times before it answers. */
  readonly auditFailures?: number;
  /** Calls already in the log, oldest first. */
  readonly calls?: readonly Omit<AuditRow, 'atMs'>[];
}

/** The tools the fake engine has when the options name none. */
export const DEFAULT_TOOLS: readonly MemoryTool[] = [
  { name: 'search_events', title: 'Search events', description: 'Searches the open logs for events matching a query.' },
  { name: 'errors_around', title: 'Errors around a time', description: 'Lists the errors in a window around a time.' },
  { name: 'incident_summary', title: 'Incident summary', description: 'Summarises what happened around an incident.' },
  { name: 'export_report', title: 'Export a report', description: 'Writes a report file to disk.', action: true },
];

const DEFAULT_SETTINGS: McpSettings = { enabled: false, resources: true, disabledTools: [], allowedActions: [] };
const T0 = 1_700_000_000_000;

/**
 * The AI access settings held in memory, playing `paddock-mcp` (browser build, demos, tests): the same refusals in spirit, a key
 * that is returned once and kept as a hash-like marker only, an audit log that grows by `recordCall`. The engine is the
 * reference; this only mimics its contract.
 */
export class MemoryMcpBackend extends McpBackend {
  private readonly listeners = new Set<(e: McpEvent) => void>();
  private readonly tools: readonly MemoryTool[];
  private settings: McpSettings;
  private keys: { id: number; client: string; createdMs: number; revoked: boolean }[] = [];
  private rows: AuditRow[] = [];
  private getFailures: number;
  private auditFailures: number;
  private nextKey = 1;
  private clock = T0;
  /** Everything the fake engine ever handed out (for tests that check a key is shown once and nowhere else). */
  readonly issued: string[] = [];

  constructor(private readonly options: MemoryMcpOptions = {}) {
    super();
    this.tools = options.tools ?? DEFAULT_TOOLS;
    this.settings = options.settings ?? DEFAULT_SETTINGS;
    this.getFailures = options.getFailures ?? 0;
    this.auditFailures = options.auditFailures ?? 0;
    for (const c of options.calls ?? []) this.rows.push({ ...c, atMs: (this.clock += 1000) });
  }

  private exposed(t: MemoryTool): boolean {
    const s = this.settings;
    if (!s.enabled || s.disabledTools.includes(t.name)) return false;
    return t.action ? s.allowedActions.includes(t.name) : true;
  }

  /** The listener the page would show: off, waiting for a key, or listening on a made-up port. */
  private listener(): McpView['listener'] {
    if (!this.settings.enabled) return { kind: 'off' };
    if (!this.keys.some((k) => !k.revoked)) return { kind: 'noKey' };
    return { kind: 'listening', port: 49152, discoveryFile: 'mcp-listener.json' };
  }

  private view(): McpView {
    const tools: ToolInfo[] = this.tools.map((t) => ({ name: t.name, title: t.title ?? null, description: t.description, action: t.action === true, exposed: this.exposed(t) }));
    return { settings: this.settings, tools, keys: this.keys.map((k) => ({ ...k })), maxKeys: MAX_KEYS, listener: this.listener(), keysProblem: this.options.keysProblem ?? null };
  }

  private emit(): void {
    for (const l of [...this.listeners]) l({ kind: 'audit', count: this.rows.length });
  }

  /** A call from a client, as the server would record it (for demos and tests). */
  recordCall(call: Omit<AuditRow, 'atMs'>): void {
    this.rows.push({ ...call, atMs: (this.clock += 1000) });
    this.emit();
  }

  async get(): Promise<McpView> {
    if (this.getFailures > 0) {
      this.getFailures--;
      throw { code: 'storeUnavailable', message: 'The AI access settings could not be read. Check the settings file and try again.', retryable: true } satisfies McpError;
    }
    return this.view();
  }

  async save(settings: McpSettings): Promise<McpView> {
    if (this.options.saveError) throw this.options.saveError;
    const known = new Set(this.tools.map((t) => t.name));
    const actions = new Set(this.tools.filter((t) => t.action).map((t) => t.name));
    const bad = settings.disabledTools.find((n) => !known.has(n));
    if (bad) throw { code: 'invalid', message: `There is no tool named ${bad}. Reload the page and try again.`, retryable: false } satisfies McpError;
    const notAction = settings.allowedActions.find((n) => !actions.has(n));
    if (notAction) throw { code: 'invalid', message: `${notAction} is not an action tool, so it needs no permission.`, retryable: false } satisfies McpError;
    this.settings = { ...settings, disabledTools: [...settings.disabledTools], allowedActions: [...settings.allowedActions] };
    return this.view();
  }

  async issueKey(client: string): Promise<IssuedMcpKey> {
    if (this.options.issueError) throw this.options.issueError;
    const name = client.trim();
    if (!name || name.length > 64) throw { code: 'invalid', message: 'Name the client in 1 to 64 characters, for example "Pitwall".', retryable: false } satisfies McpError;
    if (this.keys.filter((k) => !k.revoked).length >= MAX_KEYS) throw { code: 'invalid', message: `${MAX_KEYS} keys are in use. Revoke one you no longer need first.`, retryable: false } satisfies McpError;
    const id = this.nextKey++;
    const key = `pmk_${id.toString(16).padStart(8, '0')}${'0123456789abcdef'.repeat(4).slice(0, 56)}`;
    this.keys.push({ id, client: name, createdMs: (this.clock += 1000), revoked: false });
    this.issued.push(key);
    return { id, client: name, key };
  }

  async revokeKey(id: number): Promise<McpView> {
    if (this.options.revokeError) throw this.options.revokeError;
    const key = this.keys.find((k) => k.id === id);
    if (!key) throw { code: 'invalid', message: 'That key does not exist any more. Reload the page.', retryable: false } satisfies McpError;
    key.revoked = true;
    return this.view();
  }

  async audit(): Promise<AuditRow[]> {
    if (this.auditFailures > 0) {
      this.auditFailures--;
      throw { code: 'failed', message: 'The activity log could not be read.', retryable: true } satisfies McpError;
    }
    return [...this.rows].reverse();
  }

  async clearAudit(): Promise<void> {
    this.rows = [];
    this.emit();
  }

  async listen(handler: (event: McpEvent) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return () => void this.listeners.delete(handler);
  }
}
