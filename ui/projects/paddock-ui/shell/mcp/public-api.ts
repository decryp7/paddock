/**
 * `@decryp7/paddock-ui/shell/mcp`: the AI access (MCP) settings (plan X22, ADR 0007) over `paddock-mcp`'s Tauri adapter
 * (`mcp_get`, `mcp_save`, `mcp_issue_key`, `mcp_revoke_key`, `mcp_audit`, `mcp_audit_clear`, and the audit changes on
 * `paddock://mcp`): the `McpBackend` port (Tauri, and in-memory), the per-window store and the Settings > AI access (MCP) page and
 * dialog definitions.
 *
 * The page never depends on anything but the engine (rule 19). The server is off until the user turns it on; tools are
 * read-only, and an action tool needs its own, separately labelled permission. A client key is issued in the key dialog's window
 * and shown there once: it is in no store, event, log or URL. The activity log is the shared audit viewer (`PdMcpAudit`).
 *
 * The page (`PdMcpPage`, `@decryp7/paddock-ui/shell/mcp/page`) and the dialogs (`PdMcpKeyDialog` and the revoke confirmation
 * `PdMcpRevokeDialog`, `@decryp7/paddock-ui/shell/mcp/dialog`) are chunks of their own that only their windows load.
 *
 * ```ts
 * export const MCP_PAGE = defineMcpSettingsPage({
 *   providers: () => import('./mcp.providers').then((m) => m.MCP_PROVIDERS),
 *   load: () => import('@decryp7/paddock-ui/shell/mcp/page').then((m) => m.PdMcpPage),
 * });
 * export const MCP_KEY_DIALOG = defineMcpKeyDialog({
 *   providers: () => import('./mcp.providers').then((m) => m.MCP_KEY_PROVIDERS),
 *   load: () => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpKeyDialog),
 * });
 * // the registry: settingsPages: [..., MCP_PAGE], dialogs: [..., MCP_KEY_DIALOG, MCP_REVOKE_DIALOG];
 * // the Settings dialog: commands [...SETTINGS_DIALOG_COMMANDS, ...MCP_SETTINGS_COMMANDS, 'dialog_open']
 * ```
 */
export { McpBackend, TauriMcpBackend, type IssuedMcpKey, type McpIpc, type Unlisten } from './src/backend';
export { MemoryMcpBackend, DEFAULT_TOOLS, type MemoryMcpOptions, type MemoryTool } from './src/memory';
export { parseMcpView, parseMcpEvent, parseAuditRows, parseIssuedKey, parseSettings, asMcpError, mcpMessage } from './src/wire';
export { McpStore, type McpStoreType, type McpSaveStatus, type McpAuditStatus } from './src/mcp.store';
export {
  MCP_SETTINGS_PAGE,
  MCP_KEY_DIALOG_ID,
  MCP_REVOKE_DIALOG_ID,
  MCP_SETTINGS_COMMANDS,
  MCP_KEY_DIALOG_COMMANDS,
  defineMcpSettingsPage,
  defineMcpKeyDialog,
  defineMcpRevokeDialog,
  mcpContributions,
  type McpSettingsPageOptions,
  type McpKeyDialogOptions,
  type McpRevokeDialogOptions,
} from './src/mcp.contributions';
export { provideShellMcp, provideMcpBackend, type ShellMcpOptions } from './src/provide-mcp';
// The wire types, the event and the command names (generated from paddock-mcp's adapter).
export * from './generated/mcp';
export { MCP_COMMANDS, type McpCommand } from './generated/mcp-commands';
