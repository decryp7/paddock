/** What each dialog is opened with in the dialog e2e specs and its screenshots (the first of a list is the default). */
export const FIXTURES: Record<string, unknown[]> = {
  'pd.confirm': [
    { heading: 'Delete Qwen3.5 9B?', message: 'qwen3.5-9b-q4_k_m.gguf (5.6 GB) is removed from disk. You can download it again later.', confirm: { label: 'Delete model', destructive: true } },
    { heading: 'Roll back to Vulkan b11669?', message: 'Vulkan b11669 is pinned for all models until you unpin it. Models that are loaded keep running; they use it from their next load.', confirm: { label: 'Roll back' } },
    { heading: 'Delete 3 unused models?', message: 'Qwen3.5 9B, Gemma 4 12B Instruct with a rather long name, Phi 5 mini — 18.4 GB in total. You can download them again later. '.repeat(3), confirm: { label: 'Delete models', destructive: true } },
  ],
  'pw.mcp-key': [{ app: 'Dyno' }],
  'pw.key-secret': [{ appName: 'Raycast', secret: 'pw_Xk7fQ2mR9vLc4Tn8bJ1wHs6pYd3Ge0Ua5Zi2Ko9Nq4Fr', prefix: 'pw_Xk7fQ2mR' }, { appName: 'A rather long application name for testing', secret: 'pw_Xk7fQ2mR9vLc4Tn8bJ1wHs6pYd3Ge0Ua5Zi2Ko9Nq4Fr', prefix: 'pw_Xk7fQ2mR' }],
  'pw.service-consent': [{ platform: 'macos' }, { platform: 'windows' }, { platform: 'linux' }],
};

/** The dialog contract files Pitwall registers: its own (`src-tauri/src/dialogs.rs`) and the shell's About and confirmation. */
const UI = '../../../ui/projects/paddock-ui/shell';
export const CONTRACT_FILES = ['src/app/generated/dialog-contracts.ts', ...['about', 'confirm'].map((p) => `${UI}/${p}/generated/dialog-contracts.ts`)];

/** The first fixture of every dialog. */
export const INPUTS: Record<string, unknown> = Object.fromEntries(Object.entries(FIXTURES).map(([id, list]) => [id, list[0] ?? {}]));
