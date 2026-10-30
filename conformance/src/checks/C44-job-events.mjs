// C44: an app never listens to the shell's jobs event (`paddock://jobs`) itself and never talks to the shell's jobs backend
// outside its provider setup. The shell's JobsStore (@decryp7/paddock-ui/shell/jobs) is the one listener of a window: it
// announces a job's end as a notice, and `JobFollow` / `JobsStore.follow` let a page or dialog wait for its own job. An app
// listener next to it announces the same end twice (the Spotter and Pitcrew shell moves both did). Share first (rule 42):
// the app waits through the shell's job follow.
//   - the literal 'paddock://jobs' anywhere in app code;
//   - JOBS_EVENT, only when imported from @decryp7/paddock-ui (also `as` aliases and `import * as ns`): an app's own constant of
//     that name (Debrief's 'debrief://jobs') is its own business;
//   - a LISTEN on the shell's jobs backend (JobsBackend / TauriJobsBackend / MemoryJobsBackend imported from paddock-ui): a
//     variable assigned from `inject(Backend)` or `new Backend(...)` that later has `.listen(` called, `new Backend(..).listen(`,
//     or `inject(JobsBackend)` itself outside provider wiring (`provide*.ts`, `*.config.ts`, `main.ts`). Constructing a backend
//     or returning one from a provider factory (`jobs: { backend: () => new TauriJobsBackend(ipc) }`, a `jobsBackend()` engine
//     factory, a mock backend's `new MemoryJobsBackend()`) is the shell's documented wiring and is not flagged.
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
  const providerFile = PROVIDER_FILE.test(file);
  const backendLocals = [...names].filter(([, imp]) => BACKENDS.has(imp)).map(([l]) => l);
  for (const n of namespaces) for (const b of BACKENDS) backendLocals.push(`${n}.${b}`);
  const nsEvent = [...namespaces].map((n) => `${n}.JOBS_EVENT`);
  // Import and re-export statements declare, they do not use: blank them but keep the line numbers.
  const code = text.replace(/^[ \t]*(?:import|export)\b[^;'"]*?\bfrom\s*['"][^'"]+['"];?/gm, (m) => m.replace(/[^\n]/g, ' '));
  const esc = (id) => id.replace(/[.$]/g, '\\$&');
  const alt = backendLocals.map(esc).join('|');
  // Variables holding a backend: `x = inject(B)` or `x = new B(...)` (also `this.x`, `private readonly x`).
  const held = new Set();
  if (alt) for (const m of code.matchAll(new RegExp(`([\\w$]+)\\s*(?::[^=;]+)?=\\s*(?:inject\\(\\s*(?:${alt})\\b|new\\s+(?:${alt})\\b)`, 'g'))) held.add(m[1]);
  code.split('\n').forEach((line, i) => {
    if (/^\s*(?:\/\/|\*|\/\*)/.test(line)) return;
    const has = (id) => new RegExp(`(?<![\\w$.])${esc(id)}(?![\\w$])`).test(line);
    if (LITERAL.test(line) || eventLocals.some(has) || nsEvent.some(has)) {
      out.push({ line: i + 1, kind: 'event' });
      return;
    }
    if (!alt) return;
    const injects = !providerFile && new RegExp(`\\binject\\(\\s*(?:${alt})\\b`).test(line);
    const listens = [...held].some((v) => new RegExp(`(?<![\\w$])(?:this\\.)?${esc(v)}\\.listen\\(`).test(line)) || new RegExp(`new\\s+(?:${alt})\\b[^;]*\\)\\.listen\\(`).test(line);
    if (injects || listens) out.push({ line: i + 1, kind: 'backend' });
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
            : error('C44', 42, f, u.line, 'the app listens to the shell’s jobs backend itself (or injects it outside provider wiring), a way round JobFollow (the shell’s notice and the app’s own handling both announce the end)', 'Use JobFollow.run(...) / JobsStore.follow(id); construct the backend only in provider wiring (provideShellJobs options).'),
        );
      }
    }
    return out;
  },
};
