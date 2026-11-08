import { resolve } from 'node:path';
import { commitLintMain } from './commit-lint.mjs';
import { checks } from './checks/index.mjs';
import { runHermetic } from './hermetic.mjs';
import { formatText, writeOutputs } from './report.mjs';
import { runChecks } from './runner.mjs';
import { ownPaddock } from './sync/paddock-root.mjs';
import { syncMain } from './sync/main.mjs';
import { windowWeightMain } from './window-weight/cli.mjs';

const USAGE = `usage: paddock-conformance --app <name> [--root <dir>] [--only C04,C06] [--json] [--out file] [--summary file]
                           [--strict C30,C36] [--no-graph] [--scope apps/<app>] [--bundle <dir>] [--artifacts <dir>] [--paddock <dir>] [--version vX.Y.Z]
                           [--preflight vX.Y.Z [--api url --repo owner/name]]
       paddock-conformance run [--allow-home a,b] [--keep] -- <command> [args]
       paddock-conformance commit-lint --file <path> | --range <rev-range>
       paddock-conformance sync --app <name> [--root <dir>] [--paddock <dir>] [--write]   # C28 baseline files and C23 doc blocks
       paddock-conformance window-weight [kind...] [--root <dir>] [--stats <file>] [--max <kB>] [--record]   # C29, rule 35
       paddock-conformance list`;

const VALUE = new Set(['stats', 'max', 'app', 'root', 'only', 'out', 'summary', 'bundle', 'artifacts', 'paddock', 'version', 'preflight', 'api', 'repo', 'allow-home', 'strict', 'scope']);

/** Parses `--key value` / `--flag` into an object; throws on anything unknown. */
export function parseArgs(argv) {
  const o = {};
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) throw new Error(`unexpected argument: ${a}`);
    const k = a.slice(2);
    if (k === 'json' || k === 'keep' || k === 'write' || k === 'record' || k === 'no-graph') o[k] = true;
    else if (VALUE.has(k)) {
      if (argv[i + 1] === undefined) throw new Error(`--${k} needs a value`);
      o[k] = argv[++i];
    } else throw new Error(`unknown option: ${a}`);
  }
  return o;
}

/** Entry point; returns the exit status. 0 clean, 1 findings, 2 usage. */
export async function main(argv, { log = console.log, err = console.error, env = process.env } = {}) {
  const [cmd, ...rest] = argv;
  try {
    if (!cmd || cmd === '--help' || cmd === '-h') {
      err(USAGE);
      return cmd ? 0 : 2;
    }
    if (cmd === 'list') {
      for (const c of checks) log(`${c.id}  rule ${c.rule}  ${c.title}`);
      return 0;
    }
    if (cmd === 'window-weight') {
      const kinds = [];
      const flags = [];
      for (let i = 0; i < rest.length; i++) {
        if (rest[i].startsWith('--')) {
          flags.push(rest[i]);
          if (!['--record'].includes(rest[i])) flags.push(rest[++i] ?? '');
        } else kinds.push(rest[i]);
      }
      return windowWeightMain(parseArgs(flags), kinds, { log, err });
    }
    if (cmd === 'sync') return syncMain(parseArgs(rest), { log, err });
    if (cmd === 'commit-lint') return commitLintMain(rest, process.cwd(), err);
    if (cmd === 'run') {
      const dash = rest.indexOf('--');
      if (dash < 0 || dash === rest.length - 1) throw new Error('run needs a command after --');
      const o = parseArgs(rest.slice(0, dash));
      const [c, ...a] = rest.slice(dash + 1);
      const r = await runHermetic(c, a, { allowHome: (o['allow-home'] ?? '').split(',').filter(Boolean), keep: !!o.keep });
      for (const f of r.findings) err(`::error::${f.check} ${f.message} Fix: ${f.fix}`);
      return r.status !== 0 ? r.status : r.findings.length ? 1 : 0;
    }
    const o = parseArgs(argv);
    const result = await runChecks({
      root: resolve(o.root ?? '.'),
      app: o.app,
      only: o.only ? o.only.split(',').map((s) => s.trim().toUpperCase()) : null,
      options: {
        bundle: o.bundle && resolve(o.bundle),
        artifacts: o.artifacts && resolve(o.artifacts),
        paddock: o.paddock ? resolve(o.paddock) : ownPaddock(),
        version: o.version,
        preflight: o.preflight,
        api: o.api,
        repo: o.repo,
        token: env.GITEA_TOKEN,
        // Offline local runs only (C99 layers 3 and 4 need the registry); CI never passes it. The run says so (a warning).
        graph: o['no-graph'] ? 'skipped' : undefined,
        // C99 only: judge one app's workspaces and crates/ (a release of that app, ER-31).
        scope: o.scope,
      },
    });
    writeOutputs(result, { out: o.out, summary: o.summary, env });
    log(o.json ? JSON.stringify(result, null, 2) : formatText(result));
    // `--strict C36`: the warnings of these checks fail the run too (a check that is a warning for apps may block in one repo).
    const strict = new Set((o.strict ?? '').split(',').map((x) => x.trim().toUpperCase()).filter(Boolean));
    return result.errors || result.findings.some((f) => f.severity === 'warn' && strict.has(f.check)) ? 1 : 0;
  } catch (e) {
    err(`paddock-conformance: ${e.message}`);
    return 2;
  }
}
