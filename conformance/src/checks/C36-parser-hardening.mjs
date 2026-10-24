// C36: a crate that parses untrusted input has a fuzz target (cargo-fuzz) or a never-panics test (plan X41; the AGENTS.md of
// Debrief, Pitcrew and Pitwall promise a target for every parser). A parser crate is one named in `paddock.conformance.json`
// "parserCrates", or detected: a public `parse*`/`decode*`/`from_str`/`from_bytes` function whose first parameter is text,
// bytes or a reader. It is covered when a file of the repository that uses the crate (`crate_name::`) is a fuzz target
// (`fuzz_target!`) or contains a never-panics test (`paddock_testkit::never_panics`, `corpus(`, `proptest!`). "parserSkip"
// lists crates that are no parsers (with the waiver file for anything else). Warning level.
import { warn } from '../finding.mjs';

const PARSE_FN = /\bpub\s+(?:async\s+)?fn\s+((?:parse|decode|read|deserialize|from_str|from_bytes)\w*)\s*(?:<[^>]*>)?\s*\(\s*(?:&?(?:mut\s+)?self\s*,\s*)?(?:mut\s+)?\w+\s*:\s*(&\s*str|&\s*\[\s*u8\s*\]|String|Vec<u8>|impl\s+(?:\w+::)*(?:Buf)?Read\b|&\s*mut\s+(?:dyn\s+|impl\s+)?(?:\w+::)*(?:Buf)?Read\b|&\s*Path|&\s*\[u8\])/g;
const HARDENING = /\bfuzz_target!|never_panics\s*\(|\bcorpus\s*\(|\bproptest!\s*\{|\bproptest!\s*\(/;

/** The parsing functions in a Rust source: [{ line, name }]. */
export function parseFunctions(source) {
  return [...source.matchAll(PARSE_FN)].map((m) => ({ line: source.slice(0, m.index).split('\n').length, name: m[1] }));
}

const identOf = (crate) => crate.replaceAll('-', '_');

export default {
  id: 'C36',
  title: 'Parser crates have a fuzz target or a never-panics test',
  rule: 40,
  run(ctx) {
    const files = ctx.files();
    const crates = [];
    for (const f of files.filter((x) => x.endsWith('Cargo.toml'))) {
      const name = /\[package\][^[]*?\bname\s*=\s*"([^"]+)"/s.exec(ctx.read(f) ?? '')?.[1];
      if (!name || /(^|\/)fuzz\//.test(f)) continue;
      crates.push({ name, dir: f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : '' });
    }
    const skip = new Set(ctx.config.parserSkip ?? []);
    const named = new Set(ctx.config.parserCrates ?? []);
    const rs = files.filter((f) => f.endsWith('.rs'));
    const hardening = rs.filter((f) => HARDENING.test(ctx.read(f) ?? ''));
    const out = [];
    for (const c of crates) {
      if (skip.has(c.name)) continue;
      // A file belongs to the nearest crate above it: the demo agent inside the demo app is the agent's, not the app's.
      const owner = (f) => crates.filter((x) => x.dir === '' || f.startsWith(`${x.dir}/`)).sort((a, b) => b.dir.length - a.dir.length)[0];
      const inCrate = (f) => owner(f) === c;
      const sources = rs.filter((f) => inCrate(f) && /(^|\/)src\//.test(f) && !/(^|\/)(?:tests?|benches)(\.rs|\/)/.test(f));
      const found = sources.flatMap((f) => parseFunctions(ctx.read(f) ?? '').map((p) => ({ ...p, file: f })));
      if (!found.length && !named.has(c.name)) continue;
      const ident = new RegExp(`\\b${identOf(c.name)}::`);
      const covered = hardening.some((f) => inCrate(f) || ident.test(ctx.read(f) ?? ''));
      if (covered) continue;
      const first = found[0];
      out.push(warn('C36', 40, first ? first.file : `${c.dir ? `${c.dir}/` : ''}Cargo.toml`, first?.line ?? 1, `${c.name} parses untrusted input${first ? ` (${found.length} function(s), first ${first.name})` : ''} but has no fuzz target or never-panics test`, 'Add a fuzz target (cargo fuzz) or `assert!(paddock_testkit::never_panics(&paddock_testkit::corpus(SEED), parse).is_empty())`; list a crate that is no parser in "parserSkip".'));
    }
    return out;
  },
};
