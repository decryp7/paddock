// Release gate (ER-27, accepted ADR 0039: gates read the built artifact): the built Windows `dyno-agent.exe` must be a GUI-subsystem
// program (PE optional header Subsystem 2). A console-subsystem agent opens a visible command prompt when the per-user Run entry
// starts it at logon (Dyno 1.0.0). The `dyno` command stays a console program (Subsystem 3).
//   node scripts/check-pe-subsystem.mjs dyno-agent.exe=2 dyno.exe=3 ...   (paths relative to the current folder or absolute)
//   node scripts/check-pe-subsystem.mjs --dir <folder>                    (the release's own list, in that folder)
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

export const GUI = 2;
export const CONSOLE = 3;
/** What the release ships: the agent has no console window, the command line has one. */
export const SHIPPED_SUBSYSTEMS = ['dyno-agent.exe=2', 'dyno.exe=3', 'dyno-bench.exe=3', 'dyno-bench-gpu.exe=3'];

/** The specs for the arguments: `--dir <folder>` is the release's list in that folder. */
export function specsFor(args) {
  const i = args.indexOf('--dir');
  return i < 0 ? args : SHIPPED_SUBSYSTEMS.map((s) => `${args[i + 1].replace(/[\\/]$/, '')}/${s}`);
}

/** The Subsystem of a PE file's optional header (the same offset for PE32 and PE32+); throws a message that says what is wrong. */
export function subsystemOf(bytes) {
  const b = Buffer.from(bytes);
  if (b.length < 0x40 || b.readUInt16LE(0) !== 0x5a4d) throw new Error('not a Windows program (no MZ header)');
  const pe = b.readUInt32LE(0x3c);
  if (pe + 24 + 70 > b.length || b.readUInt32LE(pe) !== 0x4550) throw new Error('not a Windows program (no PE header)');
  const optional = pe + 24;
  const magic = b.readUInt16LE(optional);
  if (magic !== 0x10b && magic !== 0x20b) throw new Error('not a Windows program (unknown optional header)');
  return b.readUInt16LE(optional + 68);
}

/** The problems of `specs` (`path=subsystem`), read with `read(path)`; empty when all are as expected. */
export function problems(specs, read = readFileSync) {
  const out = [];
  for (const spec of specs) {
    const [path, want] = spec.split('=');
    try {
      const got = subsystemOf(read(path));
      if (got !== Number(want)) out.push(`${path} has PE subsystem ${got}, expected ${want} (2 = GUI, no console window; 3 = console)`);
    } catch (e) {
      out.push(`${path}: ${e.message}`);
    }
  }
  return out;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const specs = specsFor(process.argv.slice(2));
  const found = problems(specs);
  for (const p of found) console.log(`::error::${p}. Next step: build the program with #![windows_subsystem = "windows"] (agent) and keep the CLI a console program.`);
  if (found.length === 0) console.log(`PE subsystem ok: ${specs.join(', ')}`);
  process.exit(found.length === 0 ? 0 : 1);
}
