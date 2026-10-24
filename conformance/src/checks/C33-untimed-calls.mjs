// C33: the UI never times out a running job (rule 12), and a call that is left untimed is a deliberate choice. Every entry in
// the `untimed` list of a `withCallTimeout(...)` policy (@decryp7/paddock-ui/ipc) must be declared by the app in
// `paddock.conformance.json` "untimedCommands": { "<method>": "<why: it starts a job / it waits for the user>" }. A method
// that merely takes long gets a generous `limits` entry instead. A declared command that no policy lists any more is stale.
import { error, warn } from '../finding.mjs';

/** The text between the bracket at `open` and its match (exclusive), or null. */
function balanced(text, open) {
  const pairs = { '(': ')', '[': ']', '{': '}' };
  const close = pairs[text[open]];
  let depth = 0;
  for (let i = open; i < text.length; i++) {
    if (text[i] === text[open]) depth++;
    else if (text[i] === close && --depth === 0) return text.slice(open + 1, i);
  }
  return null;
}

/** The policies in a TypeScript source: [{ line, untimed: string[] | null }] (null: the list is not a literal array of strings). */
export function untimedPolicies(source) {
  const out = [];
  for (const m of source.matchAll(/\bwithCallTimeout\s*(?:<[^>(]*>)?\s*\(/g)) {
    const open = m.index + m[0].length - 1;
    const args = balanced(source, open);
    if (args === null) continue;
    const line = source.slice(0, m.index).split('\n').length;
    const key = /\buntimed\s*:\s*/.exec(args);
    if (!key) {
      out.push({ line, untimed: [] });
      continue;
    }
    const at = key.index + key[0].length;
    if (args[at] !== '[') {
      out.push({ line, untimed: null });
      continue;
    }
    const list = balanced(args, at) ?? '';
    const items = list.split(',').map((s) => s.trim()).filter(Boolean);
    const names = items.map((s) => /^(['"`])([\w$.-]+)\1$/.exec(s)?.[2]);
    out.push({ line, untimed: names.every((n) => n !== undefined) ? names : null });
  }
  return out;
}

export default {
  id: 'C33',
  title: 'Untimed calls are declared job-starting or user-waiting commands',
  rule: 12,
  run(ctx) {
    const out = [];
    const declared = ctx.config.untimedCommands ?? {};
    const used = new Set();
    const root = `${ctx.config.appDir}/`;
    for (const f of ctx.files()) {
      if (!f.startsWith(root) || !/\.[mc]?tsx?$/.test(f) || /\.(?:spec|test)\.[mc]?tsx?$/.test(f)) continue;
      const text = ctx.read(f) ?? '';
      if (!text.includes('withCallTimeout')) continue;
      for (const p of untimedPolicies(text)) {
        if (p.untimed === null) {
          out.push(warn('C33', 12, f, p.line, 'the untimed list is not a literal array of method names, so it cannot be checked', 'Write untimed: ["method", ...] with string literals.'));
          continue;
        }
        for (const name of p.untimed) {
          used.add(name);
          const why = declared[name];
          if (why === undefined) out.push(error('C33', 12, f, p.line, `${name} is untimed but not declared in "untimedCommands" of paddock.conformance.json`, 'Declare it with the reason (it starts a job that reports through events, or it waits for the user); if it only takes long, give it a limit instead.'));
          else if (typeof why !== 'string' || why.trim().length < 8) out.push(error('C33', 12, 'paddock.conformance.json', 1, `untimedCommands.${name} needs a reason (why the call is never timed out)`, 'Write what the command does: starts a job, waits for the user, ...'));
        }
      }
    }
    for (const name of Object.keys(declared)) {
      if (!used.has(name)) out.push(warn('C33', 12, 'paddock.conformance.json', 1, `untimedCommands.${name} is declared but no withCallTimeout policy lists it`, 'Remove the stale entry.'));
    }
    return out;
  },
};
