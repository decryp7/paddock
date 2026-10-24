import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { openTree } from './util/fs.mjs';
import { parseJobs, parseSteps } from './util/workflows.mjs';

export const DEFAULT_CONFIG = {
  app: null,
  appName: null,
  docBlocks: null,
  baselineSkip: [],
  windowWeight: null,
  appDir: 'app',
  tauriDir: 'app/src-tauri',
  workflowsDir: '.gitea/workflows',
  sidecars: [],
  nonGates: [],
  commitRange: null,
};

/** `paddock.conformance.json` merged over the defaults. A file that exists but cannot be read is an error, not a default. */
export function loadConfig(root, app) {
  let fromFile = {};
  try {
    fromFile = JSON.parse(readFileSync(join(root, 'paddock.conformance.json'), 'utf8'));
  } catch (e) {
    if (e.code !== 'ENOENT') throw new Error(`paddock.conformance.json: ${e.message}`);
  }
  const config = { ...DEFAULT_CONFIG, ...fromFile };
  if (app) config.app = app;
  if (!config.app) throw new Error('the app name is needed: --app <name> or "app" in paddock.conformance.json');
  return config;
}

/** What a check sees. `options` carries the run-specific inputs (bundle, artifacts, paddock checkout, preflight, injected exec/fetch). */
export function createContext({ root, config, options = {} }) {
  const tree = openTree(root);
  let workflows = null;
  return {
    root,
    app: config.app,
    config,
    options,
    now: options.now ?? new Date(),
    tree,
    files: () => tree.files(),
    read: (p) => tree.read(p),
    exists: (p) => tree.exists(p),
    /** Workflow files: [{ file, text, jobs, steps }]. */
    workflows() {
      workflows ??= tree
        .files()
        .filter((f) => f.startsWith(`${config.workflowsDir}/`) && /\.ya?ml$/.test(f))
        .map((file) => {
          const text = tree.read(file) ?? '';
          return { file, text, jobs: parseJobs(text), steps: parseSteps(text) };
        });
      return workflows;
    },
  };
}
