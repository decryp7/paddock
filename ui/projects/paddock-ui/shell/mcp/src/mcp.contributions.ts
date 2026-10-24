import { defineDialog, defineSettingsPage, msg, type, type ContributionBatch, type DialogDef, type ProvidersLoader, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import type { McpCommand } from '../generated/mcp-commands';

// The AI access part's contributions: static data and loaders only (lazy by construction, plan §9). The page's component, the
// dialogs' components and the providers each need are chunks of their own, loaded when their window opens.

/** The id of the AI access (MCP) settings page. */
export const MCP_SETTINGS_PAGE = 'pd.mcp';

/** The id of the key dialog (stable: routes, remembered bounds, test ids). */
export const MCP_KEY_DIALOG_ID = 'pd.mcp.key';

/** The id of the revoke confirmation (stable: routes, remembered bounds, test ids). */
export const MCP_REVOKE_DIALOG_ID = 'pd.mcp.revoke';

/**
 * What the Settings window may call for this page, on top of the settings commands: the Settings dialog's contract lists these
 * too (write them out in the dialog, the contract check reads literals). `mcp_issue_key` is not among them: a key is issued
 * from the key dialog's window only, where it is shown once.
 */
export const MCP_SETTINGS_COMMANDS = ['mcp_get', 'mcp_save', 'mcp_revoke_key', 'mcp_audit', 'mcp_audit_clear'] as const satisfies readonly McpCommand[];

/** What the key dialog's window may call (its contract; Rust's guard allows exactly this). */
export const MCP_KEY_DIALOG_COMMANDS = ['mcp_issue_key'] as const satisfies readonly McpCommand[];

export interface McpSettingsPageOptions {
  /** The providers the page boots with: `provideShellMcp` with the app's backend, from a module of its own so they stay lazy. */
  readonly providers: ProvidersLoader;
  /** The page's component: `() => import('@decryp7/paddock-ui/shell/mcp/page').then((m) => m.PdMcpPage)`. */
  readonly load: NonNullable<SettingsPageDef['load']>;
}

/** The AI access (MCP) page of the Settings window. */
export function defineMcpSettingsPage(options: McpSettingsPageOptions): SettingsPageDef {
  return defineSettingsPage({
    id: MCP_SETTINGS_PAGE,
    group: 'application',
    title: msg('pd.settings.mcp', 'AI access (MCP)'),
    order: 90,
    providers: options.providers,
    load: options.load,
  });
}

export interface McpKeyDialogOptions {
  /** The providers the dialog's window boots with: `provideMcpBackend` with the app's backend. */
  readonly providers: ProvidersLoader;
  /** The dialog's component: `() => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpKeyDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The key dialog (a modal form, the credential dialog of this page): the user names the client, the engine issues a key, and
 * the dialog shows it once with Copy and Done. It resolves `true` when a key was issued, nothing when dismissed first. Opened
 * with `DialogService.open(MCP_KEY_DIALOG_ID)` from the page.
 */
export function defineMcpKeyDialog(options: McpKeyDialogOptions): DialogDef<typeof MCP_KEY_DIALOG_ID, boolean, McpCommand> {
  return defineDialog({
    id: MCP_KEY_DIALOG_ID,
    kind: 'form',
    modality: 'modal',
    requiredChoice: false,
    title: msg('pd.mcp.key.title', 'Add a client'),
    width: 480,
    result: type<boolean>(),
    commands: MCP_KEY_DIALOG_COMMANDS,
    providers: options.providers,
    load: options.load,
  });
}

export interface McpRevokeDialogOptions {
  /** The dialog's component: `() => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpRevokeDialog)`. */
  readonly load: DialogDef['load'];
}

/**
 * The revoke confirmation (rule 17: a real window): a modal decision about one key (`inputs: { keyId, client }`), one window
 * per key. It calls nothing; it resolves `true` when the user chose Revoke, `false` or nothing when they kept the key.
 */
export function defineMcpRevokeDialog(options: McpRevokeDialogOptions): DialogDef<typeof MCP_REVOKE_DIALOG_ID, boolean, McpCommand> {
  return defineDialog({
    id: MCP_REVOKE_DIALOG_ID,
    kind: 'decision',
    title: msg('pd.mcp.revoke.title', 'Revoke key'),
    width: 420,
    instanced: true,
    result: type<boolean>(),
    commands: [],
    load: options.load,
  });
}

/** The page and its dialogs as a contribution batch for the app's registry; the app also lists the page among the Settings window's pages. */
export function mcpContributions(page: SettingsPageDef, dialogs: readonly DialogDef[]): ContributionBatch {
  return { settingsPages: [page], dialogs: [...dialogs] };
}
