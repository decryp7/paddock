import { MemoryMcpBackend, TauriMcpBackend, provideMcpBackend, provideShellMcp, type McpBackend, type MemoryTool } from '@decryp7/paddock-ui/shell/mcp';
import { tauriIpc } from './ipc';

/** The tools the browser build lists: the same four the app registers (`src-tauri/src/mcp.rs`), two reads and two actions. */
export const DYNO_MCP_TOOLS: readonly MemoryTool[] = [
  { name: 'power_state', title: 'Power state', description: 'Whether Dyno is keeping the computer awake, and why a hold ended.' },
  { name: 'wol_devices', title: 'Wake-on-LAN computers', description: 'The computers saved for Wake-on-LAN and how their last wake ended.' },
  { name: 'wake_device', title: 'Wake a computer', description: 'Sends a Wake-on-LAN wake to one saved computer.', action: true },
  { name: 'keep_awake', title: 'Keep the computer awake', description: 'Keeps this computer awake for up to 12 hours.', action: true },
];

let memory: MemoryMcpBackend | null = null;

/** The AI access settings over Tauri in the app; one in-memory engine per page in the browser build (off by default). */
export function dynoMcpBackend(): McpBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriMcpBackend(ipc);
  memory ??= new MemoryMcpBackend({ tools: DYNO_MCP_TOOLS });
  return memory;
}

/** What the AI access page boots with (loaded with the page, never in the main window's first paint). */
export const MCP_PROVIDERS = [provideShellMcp({ backend: dynoMcpBackend })];

/** What the key dialog's window boots with: the backend only (the key is never state). */
export const MCP_KEY_PROVIDERS = [provideMcpBackend({ backend: dynoMcpBackend })];
