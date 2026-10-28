import { MemoryMcpBackend, TauriMcpBackend, type McpBackend, type MemoryMcpOptions } from '@decryp7/paddock-ui/shell/mcp';
import { tauriIpc } from './demo-ipc';

/**
 * What the browser build starts with (`?mcp=error|calls|many-keys|refuse|slow`; nothing by default: off, no clients, an empty log).
 * `calls` has a few recorded calls, `error` fails the first ask once, `many-keys` has the key limit nearly reached.
 */
export function fakeMcp(search: string = globalThis.location?.search ?? ''): MemoryMcpOptions {
  const wanted = new URLSearchParams(search).get('mcp');
  if (wanted === 'error') return { getFailures: 1 };
  // The key dialog's rejected state: issuing a key is refused with the engine's message.
  if (wanted === 'refuse') return { issueError: { code: 'storeUnavailable', message: 'The key could not be stored: the settings folder is read-only. Check its permissions, then try again.', retryable: true } };
  if (wanted === 'calls') {
    return {
      calls: [
        { client: 'Pitwall', tool: 'search_events', arguments: { query: '<omitted: 17 characters>', limit: 20 }, resultBytes: 4120, durationMs: 38, outcome: 'ok' },
        { client: 'Pitwall', tool: 'errors_around', arguments: { time: '2026-10-24T09:30:00Z', windowMinutes: 5 }, resultBytes: 1530, durationMs: 21, outcome: 'ok' },
        { client: 'script', tool: 'export_report', arguments: {}, resultBytes: 0, durationMs: 0, outcome: 'refused' },
      ],
    };
  }
  return {};
}

let memory: MemoryMcpBackend | null = null;

/**
 * The AI access settings over Tauri in the desktop build; in the browser, one in-memory engine per page (the Settings window and
 * the key dialog share it, as windows share the real engine).
 */
export function demoMcpBackend(): McpBackend {
  const ipc = tauriIpc();
  if (ipc) return new TauriMcpBackend(ipc);
  if (!memory) {
    const engine = new MemoryMcpBackend(fakeMcp());
    // The key dialog's busy state (`?mcp=slow`): issuing a key never answers.
    if (new URLSearchParams(globalThis.location?.search ?? '').get('mcp') === 'slow') engine.issueKey = () => new Promise(() => undefined);
    memory = engine;
    // Demo only: the browser build has no client to call a tool, so a spec (or a person in the console) plays one.
    (globalThis as { __pdDemoMcp?: MemoryMcpBackend }).__pdDemoMcp = engine;
  }
  return memory;
}
