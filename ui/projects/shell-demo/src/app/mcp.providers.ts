import { provideMcpBackend, provideShellMcp } from '@decryp7/paddock-ui/shell/mcp';
import { demoMcpBackend } from './demo-mcp';

/** What the AI access page boots with (loaded with the page, never in the main window's first paint). */
export const MCP_PROVIDERS = [provideShellMcp({ backend: demoMcpBackend })];

/** What the key dialog's window boots with: the backend only (the key is never state). */
export const MCP_KEY_PROVIDERS = [provideMcpBackend({ backend: demoMcpBackend })];
