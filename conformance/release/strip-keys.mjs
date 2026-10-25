#!/usr/bin/env node
// Strips whitespace from the Tauri signing secrets before the bundler reads them. Secrets pasted into Gitea often carry a
// trailing newline or a line break; both keys are single-line base64 ("Invalid symbol 10" otherwise).
//
//   eval "$(node conformance/release/strip-keys.mjs)"            export the stripped values in a bash step
//   node conformance/release/strip-keys.mjs --append-to "$GITHUB_ENV"   append NAME=value lines for the following steps
//
// Only names that are set and non-empty are written. Values go to stdout or the file, never to the log.

import { appendFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

/** The secrets that are single-line base64 (the key password is free text and is left alone). */
export const KEY_NAMES = ['TAURI_SIGNING_PRIVATE_KEY', 'TAURI_UPDATER_PUBKEY'];

/** `value` without spaces, tabs, carriage returns and line feeds. */
export function stripKey(value) {
  return String(value ?? '').replace(/[ \t\r\n]+/g, '');
}

/** { NAME: stripped } for the names that are set and non-empty after stripping. */
export function strippedEnv(env = process.env, names = KEY_NAMES) {
  const out = {};
  for (const n of names) {
    const v = stripKey(env[n]);
    if (v) out[n] = v;
  }
  return out;
}

/** `export NAME='value'` lines (base64 holds no quote characters; anything else is refused). */
export function exportLines(stripped) {
  return Object.entries(stripped).map(([k, v]) => {
    if (/'/.test(v)) throw new Error(`${k} contains a quote character: it is not a base64 key`);
    return `export ${k}='${v}'`;
  });
}

if (isMain(import.meta.url)) {
  try {
    const stripped = strippedEnv();
    const i = process.argv.indexOf('--append-to');
    if (i > 0) {
      const file = process.argv[i + 1];
      if (!file) throw new Error('--append-to needs a path');
      appendFileSync(file, `${Object.entries(stripped).map(([k, v]) => `${k}=${v}`).join('\n')}\n`);
    } else process.stdout.write(`${exportLines(stripped).join('\n')}\n`);
  } catch (e) {
    console.error(`strip-keys: ${e.message}`);
    process.exit(1);
  }
}
