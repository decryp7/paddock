/**
 * `@decryp7/paddock-ui/shell/mcp/page`: the Settings > AI access (MCP) page (`PdMcpPage`) and the shared audit viewer
 * (`PdMcpAudit`), split from `shell/mcp` so a main window's first paint carries none of it (plan S16). The app's registry
 * loads it: `load: () => import('@decryp7/paddock-ui/shell/mcp/page').then((m) => m.PdMcpPage)`.
 */
export { PdMcpPage } from './src/mcp-page';
export { PdMcpAudit } from './src/mcp-audit';
