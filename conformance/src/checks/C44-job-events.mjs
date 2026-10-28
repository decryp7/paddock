// C44: an app never listens to the shell's jobs event (`paddock://jobs`) itself and never talks to the shell's jobs backend
// outside its provider setup. The shell's JobsStore (@decryp7/paddock-ui/shell/jobs) is the one listener of a window: it
// announces a job's end as a notice, and `JobFollow` / `JobsStore.follow` let a page or dialog wait for its own job. An app
// listener next to it announces the same end twice (the Spotter and Pitcrew shell moves both did). Share first (rule 42):
// the app waits through the shell's job follow.
//   - the literal 'paddock://jobs' anywhere in app code;
//   - JOBS_EVENT, only when imported from @decryp7/paddock-ui (also `as` aliases and `import * as ns`): an app's own constant of
//     that name (Debrief's 'debrief://jobs') is its own business;
//   - JobsBackend / TauriJobsBackend / MemoryJobsBackend imported from @decryp7/paddock-ui outside provider setup
//     (`provide*.ts`, `*.config.ts`, `main.ts`), the realistic way round JobFollow.
import { error } from '../finding.mjs';

const LITERAL = /paddock:\/\/jobs\b/;
const BACKENDS = new Set(['JobsBackend', 'TauriJobsBackend', 'MemoryJobsBackend']);
const PROVIDER_FILE = /(?:^|\/)(?:provide[\w.-]*|[\w.-]*\.config|main)\.[mc]?tsx?$/;

/** The shell names an app file imports: [{ names: Map<local, imported>, namespaces: Set<local> }] from @decryp7/paddock-ui*. */
export function shellImports(text) {
  const names = new Map();
  const namespaces = new Set();
  for (const m of text.matchAll(/import\s+(?:type\s+)?([\s\S]*?)\s*from\s*['"](@decryp7\/paddock-ui[^'"]*)['"]/g)) {
    const clause = m[1];
    const ns = /\*\s+as\s+([\w$]+)/.exec(clause);
    if (ns) namespaces.add(ns[1]);
    const braces = /\{([\s\S]*)\}/.exec(clause)?.[1] ?? '';
    for (const part of braces.split(',')) {
      const spec = part.trim().replace(/^type\s+/, '');
      if (!spec) continue;
      const [imported, local] = spec.split(/\s+as\s+/);
      names.set((local ?? imported).trim(), imported.trim());
    }
  }
  return { names, namespaces };
}

/** Findings of one file: [{ line, kind: 'event' | 'backend' }]. */
export function jobUses(text, file) {
  const out = [];
  const { names, namespaces } = shellImports(text);
  const eventLocals = [...names].filter(([, imp]) => imp === 'JOBS_EVENT').map(([l]) => l);
  const backendLocals = PROVIDER_FILE.test(file) ? [] : [...names].filter(([, imp]) => BACKENDS.has(imp)).map(([l]) => l);
  const nsEvent = [...namespaces].map((n) => `${n}.JOBS_EVENT`);
  const nsBackends = PROVIDER_FILE.test(file) ? [] : [...namespaces].flatMap((n) => [...BACKENDS].map((b) => `${n}.${b}`));
  // Import and re-export statements declare, they do not use: blank them but keep the line numbers.
  const code = text.replace(/^[ \t]*(?:import|export)\b[^;'"]*?\bfrom\s*['"][^'"]+['"];?/gm, (m) => m.replace(/[^\n]/g, ' '));
  code.split('\n').forEach((line, i) => {
    if (/^\s*(?:\/\/|\*|\/\*)/.test(line)) return;
    const has = (id) => new RegExp(`(?<![\\w$.])${id.replace(/\./g, '\\.')}(?![\\w$])`).test(line);
    if (LITERAL.test(line) || eventLocals.some(has) || nsEvent.some(has)) out.push({ line: i + 1, kind: 'event' });
    else if (backendLocals.some(has) || nsBackends.some(has)) out.push({ line: i + 1, kind: 'backend' });
  });
  return out;
}

export default {
  id: 'C44',
  title: 'Apps wait for jobs through the shell’s job follow, never a direct paddock://jobs listener or backend',
  rule: 42,
  run(ctx) {
    const out = [];
    const root = `${ctx.config.appDir}/`;
    for (const f of ctx.files()) {
      if (!f.startsWith(root) || !/\.[mc]?tsx?$/.test(f) || /\.(?:spec|test)\.[mc]?tsx?$/.test(f) || f.includes('/node_modules/')) continue;
      for (const u of jobUses(ctx.read(f) ?? '', f)) {
        out.push(
          u.kind === 'event'
            ? error('C44', 42, f, u.line, 'the app listens to the shell’s jobs event (paddock://jobs) itself, so a job’s end is announced twice (its listener and the shell’s notice)', 'Start the work with JobFollow.run(...) / JobsStore.follow(id) from @decryp7/paddock-ui/shell/jobs and wait for its result; only paddock-ui names the event.')
            : error('C44', 42, f, u.line, 'the app uses the shell’s jobs backend outside its provider setup, a way round JobFollow (the shell’s notice and the app’s own handling both announce the end)', 'Use JobFollow.run(...) / JobsStore.follow(id); keep the backend in provideShellJobs / a provide*.ts file.'),
        );
      }
    }
    return out;
  },
};
