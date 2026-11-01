// Runs a native program for a CI step and, when it fails, says how: the raw exit code in decimal and hex, the NTSTATUS name
// when Windows ended it with one, the signal on Unix, and the last lines of its stderr.
//
// Why not plain bash: Git Bash (MSYS2/Cygwin) maps every NTSTATUS code at or above 0xC0000000 that it has no signal for
// (0xC0000409 fast-fail/abort, 0xC00000FD stack overflow, 0xC0000374 heap corruption, ...) to exit 127, so the step only ever
// saw "127" (Cygwin winsup/cygwin/pinfo.cc, pinfo::status_exit). Node reads the code with GetExitCodeProcess (libuv
// src/win/process.c), so here it arrives whole.
//
// Usage: node run-native.mjs [--code-file <path>] [--tail <lines>] -- <program> [args...]
// Output is streamed live. Exits 0 when the program did, otherwise 1 (never the raw code: bash would map it again); the raw
// code goes to --code-file as one line `<decimal> <0xHEX> <name>` (or `signal <NAME>`), for the step's own message.
import { spawn } from 'node:child_process';
import { writeFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** NTSTATUS codes a crashing Windows program ends with, by name (ntstatus.h). */
export const NTSTATUS = new Map([
  [0xc0000005, 'STATUS_ACCESS_VIOLATION'],
  [0xc0000017, 'STATUS_NO_MEMORY'],
  [0xc000001d, 'STATUS_ILLEGAL_INSTRUCTION'],
  [0xc0000135, 'STATUS_DLL_NOT_FOUND'],
  [0xc0000139, 'STATUS_ENTRYPOINT_NOT_FOUND'],
  [0xc000013a, 'STATUS_CONTROL_C_EXIT'],
  [0xc0000142, 'STATUS_DLL_INIT_FAILED'],
  [0xc00000fd, 'STATUS_STACK_OVERFLOW'],
  [0xc0000374, 'STATUS_HEAP_CORRUPTION'],
  [0xc0000409, 'STATUS_STACK_BUFFER_OVERRUN (fast-fail: abort() or a Rust panic=abort)'],
  [0xc0000420, 'STATUS_ASSERTION_FAILURE'],
]);

/** One line describing how a program ended: `<decimal> <0xHEX> <name>` for a code, `signal <NAME>` for a Unix signal. */
export function describeExit(code, signal) {
  if (signal) return `signal ${signal}`;
  const n = Number(code) >>> 0;
  const hex = `0x${n.toString(16).toUpperCase().padStart(8, '0')}`;
  const name = NTSTATUS.get(n) ?? (n >= 0xc0000000 ? 'NTSTATUS error' : 'exit code');
  return `${n} ${hex} ${name}`;
}

/** Whether an exit looks like a crash (a signal or an NTSTATUS error) rather than the program reporting a failure. */
export function isCrash(code, signal) {
  return Boolean(signal) || Number(code) >>> 0 >= 0xc0000000;
}

/** Keeps the last `max` lines of a stream (bounded memory, whatever the program prints). */
export function lineTail(max) {
  let lines = [];
  let partial = '';
  return {
    push(chunk) {
      const parts = (partial + chunk).split(/\r?\n/);
      partial = parts.pop() ?? '';
      lines.push(...parts);
      if (lines.length > max) lines = lines.slice(-max);
    },
    lines() {
      return partial ? [...lines, partial].slice(-max) : lines;
    },
  };
}

function parseArgs(argv) {
  const opts = { codeFile: '', tail: 40, command: [] };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--') {
      opts.command = argv.slice(i + 1);
      break;
    }
    if (a === '--code-file') opts.codeFile = argv[++i] ?? '';
    else if (a === '--tail') opts.tail = Math.max(1, Math.min(500, Number(argv[++i]) || 40));
    else throw new Error(`run-native: unknown argument ${a}`);
  }
  if (opts.command.length === 0) throw new Error('run-native: no program given (usage: run-native.mjs [--code-file f] -- program args...)');
  return opts;
}

if (isMain(import.meta.url)) {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (e) {
    console.error(e.message);
    process.exit(2);
  }
  const tail = lineTail(opts.tail);
  const child = spawn(opts.command[0], opts.command.slice(1), { stdio: ['inherit', 'inherit', 'pipe'] });
  child.stderr.setEncoding('utf8');
  child.stderr.on('data', (chunk) => {
    process.stderr.write(chunk);
    tail.push(chunk);
  });
  const finish = (code, signal) => {
    if (code === 0 && !signal) process.exit(0);
    const how = describeExit(code ?? 0, signal);
    if (opts.codeFile) writeFileSync(opts.codeFile, `${how}\n`);
    const name = opts.command[0];
    console.error(`run-native: ${name} ended with ${how}${isCrash(code, signal) ? ' (a crash, not an error it reported)' : ''}`);
    const last = tail.lines().filter((l) => l.trim() !== '');
    console.error(last.length ? `run-native: last ${last.length} line(s) of ${name}'s stderr:\n  ${last.join('\n  ')}` : `run-native: ${name} wrote nothing to stderr`);
    process.exit(1);
  };
  child.on('error', (e) => {
    if (opts.codeFile) writeFileSync(opts.codeFile, `not started ${e.code ?? ''}\n`);
    console.error(`run-native: could not start ${opts.command[0]}: ${e.message}`);
    process.exit(1);
  });
  child.on('close', finish);
}
