/**
 * `@decryp7/paddock-ui/shell/mcp/dialog`: the AI access dialogs' content, split from `shell/mcp` so only their windows load it:
 * the key dialog (`PdMcpKeyDialog`) and the revoke confirmation (`PdMcpRevokeDialog`). The app's registry loads them:
 * `load: () => import('@decryp7/paddock-ui/shell/mcp/dialog').then((m) => m.PdMcpKeyDialog)`.
 */
export { PdMcpKeyDialog } from './src/mcp-key-dialog';
export { PdMcpRevokeDialog } from './src/mcp-revoke-dialog';
