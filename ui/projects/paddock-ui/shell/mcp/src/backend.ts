import { MCP_EVENT, type AuditRow, type McpEvent, type McpSettings, type McpView } from '../generated/mcp';
import type { McpCommand } from '../generated/mcp-commands';
import { parseAuditRows, parseIssuedKey, parseMcpEvent, parseMcpView } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/** A client key as `mcp_issue_key` answers: the key is shown once and never again. */
export interface IssuedMcpKey {
  readonly id: number;
  readonly client: string;
  readonly key: string;
}

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface McpIpc {
  invoke<T>(command: McpCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The AI access (MCP) port (`paddock_mcp::tauri`). The engine validates and acts; this only asks and shows. Every command may
 * reject with a `McpError` whose message is for the user as it is.
 */
export abstract class McpBackend {
  /** The settings, the tools and the clients' keys (never the keys themselves). */
  abstract get(): Promise<McpView>;
  /** Saves the settings; rejects `invalid` with the engine's message. Answers the view as stored. */
  abstract save(settings: McpSettings): Promise<McpView>;
  /** Issues a key for `client`. Called from the key dialog's window only; the key is in the answer and nowhere else. */
  abstract issueKey(client: string): Promise<IssuedMcpKey>;
  /** Revokes the key `id`; answers the view as stored. */
  abstract revokeKey(id: number): Promise<McpView>;
  /** The recent calls, newest first. */
  abstract audit(): Promise<AuditRow[]>;
  /** Empties the audit log. */
  abstract clearAudit(): Promise<void>;
  /** Every event the engine sends until the returned function is called. */
  abstract listen(handler: (event: McpEvent) => void): Promise<Unlisten>;
}

/** The AI access settings over Tauri. Payloads are parsed at this edge. */
export class TauriMcpBackend extends McpBackend {
  constructor(private readonly ipc: McpIpc) {
    super();
  }

  async get(): Promise<McpView> {
    return this.view(await this.ipc.invoke<unknown>('mcp_get'));
  }

  async save(settings: McpSettings): Promise<McpView> {
    return this.view(await this.ipc.invoke<unknown>('mcp_save', { settings }));
  }

  async issueKey(client: string): Promise<IssuedMcpKey> {
    const key = parseIssuedKey(await this.ipc.invoke<unknown>('mcp_issue_key', { client }));
    if (!key) throw new Error('The engine did not send a key.');
    return key;
  }

  async revokeKey(id: number): Promise<McpView> {
    return this.view(await this.ipc.invoke<unknown>('mcp_revoke_key', { id }));
  }

  async audit(): Promise<AuditRow[]> {
    const rows = parseAuditRows(await this.ipc.invoke<unknown>('mcp_audit'));
    if (!rows) throw new Error('The engine sent no valid activity log.');
    return rows;
  }

  async clearAudit(): Promise<void> {
    await this.ipc.invoke<unknown>('mcp_audit_clear');
  }

  listen(handler: (event: McpEvent) => void): Promise<Unlisten> {
    return this.ipc.listen(MCP_EVENT, (payload) => {
      const event = parseMcpEvent(payload);
      if (event) handler(event);
    });
  }

  private view(raw: unknown): McpView {
    const view = parseMcpView(raw);
    if (!view) throw new Error('The engine sent no valid AI access settings.');
    return view;
  }
}
