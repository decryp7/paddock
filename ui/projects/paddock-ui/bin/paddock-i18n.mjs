#!/usr/bin/env node
// paddock-i18n: the translation files of a Paddock app (Paddock ADR 0012 §6, plan S15). Messages come from two places:
// `$localize` / template `i18n` texts (extracted from the build by `localize-extract --format json`, or `$localize` with an
// `@@id` scanned from the sources) and the contributions' `msg('id', 'Text')` (scanned from the sources, or read from the
// manifest's `messages` section).
//
//   paddock-i18n catalog --out messages.xlf [--json extracted.json]... [--src dir]... [--manifest m.json]... [--prefix pd.] [--check]
//       writes the XLIFF 1.2 catalog (sorted, no locations): every message once; a message id with two texts fails, and
//       so does a msg() call whose arguments are not two string literals.
//   paddock-i18n merge --locale <tag> --out messages.<tag>.json <input>... [--check]
//       writes the runtime JSON the app's loader imports. Inputs: runtime JSONs (`{ locale, translations }`, e.g. the
//       package's i18n/messages.pd.en-XA.json) and XLIFF files (their <target>s; for the pseudo-locale en-XA, the
//       generated pseudo text of every <source>). An id translated twice, differently, fails.
//   paddock-i18n build [--config paddock-i18n.json] [--no-extract] [--check]
//       the whole build an app declares in paddock-i18n.json (see i18n-build.mjs): the extraction from the built output,
//       every catalog, the pseudo-locale and every translated locale's runtime JSON. `--no-extract` reads the committed
//       catalogs instead of extracting (no build needed).
//
// `--check` writes nothing and fails when the file on disk differs (CI: "run npm run i18n and commit the result").
import { existsSync, readdirSync, readFileSync, realpathSync, statSync, writeFileSync } from 'node:fs';
import { join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import {
  buildCatalog,
  mergeTranslations,
  messagesFromJson,
  messagesFromManifest,
  parseXliff,
  pseudoTranslations,
  runtimeJson,
  scanLocalizeCalls,
  scanMsgCalls,
  targetTranslations,
  writeXliff,
} from './i18n-messages.mjs';
import { loadConfig, runBuild } from './i18n-build.mjs';

export const PSEUDO_LOCALE = 'en-XA';

/** Parses `--name value` pairs (repeatable) and positional arguments. */
export function parseArgs(argv) {
  const flags = new Map();
  const positional = [];
  const BOOLEAN = new Set(['check', 'no-extract']);
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    if (!a.startsWith('--')) {
      positional.push(a);
      continue;
    }
    const name = a.slice(2);
    const value = BOOLEAN.has(name) ? 'true' : argv[++i];
    if (value === undefined) throw new Error(`--${name} needs a value`);
    flags.set(name, [...(flags.get(name) ?? []), value]);
  }
  return { flags, positional };
}

/** Every non-spec TypeScript file under `dir`. */
function sourceFiles(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    if (name === 'node_modules' || name.startsWith('.')) continue;
    const p = join(dir, name);
    if (statSync(p).isDirectory()) out.push(...sourceFiles(p));
    else if (name.endsWith('.ts') && !name.endsWith('.spec.ts') && !name.endsWith('.d.ts')) out.push(p);
  }
  return out.sort();
}

/**
 * The `msg()` and `$localize` messages of every source under `dirs` whose id has `prefix`; a dynamic `msg()` or a
 * `$localize` without an `@@id` fails.
 */
export function scanSources(dirs, prefix) {
  const messages = [];
  const problems = [];
  for (const dir of dirs) {
    for (const file of sourceFiles(dir)) {
      const text = readFileSync(file, 'utf8');
      const at = relative(process.cwd(), file);
      const { messages: found, dynamic } = scanMsgCalls(text);
      const { messages: localized, unnamed } = scanLocalizeCalls(text);
      messages.push(...[...found, ...localized].filter((m) => !prefix || m.id.startsWith(prefix)));
      for (const line of dynamic) problems.push(`${at}:${line}: msg() needs two string literals (its text cannot be extracted)`);
      for (const line of unnamed) problems.push(`${at}:${line}: $localize needs an @@id (:@@ns.name:Text)`);
    }
  }
  if (problems.length) throw new Error(problems.join('\n'));
  return messages;
}

/** The catalog's XLIFF text for the `catalog` command's flags. */
export function catalogText(flags) {
  const prefix = flags.get('prefix')?.[0] ?? '';
  const lists = [
    ...(flags.get('json') ?? []).map((f) => messagesFromJson(JSON.parse(readFileSync(f, 'utf8')))),
    ...(flags.get('xlf') ?? []).map((f) => parseXliff(readFileSync(f, 'utf8'))),
    ...(flags.get('manifest') ?? []).map((f) => messagesFromManifest(JSON.parse(readFileSync(f, 'utf8')))),
    scanSources(flags.get('src') ?? [], prefix),
  ];
  const catalog = buildCatalog(lists, prefix);
  if (!catalog.length) throw new Error('the catalog would be empty: check --json, --src, --manifest and --prefix');
  return writeXliff(catalog);
}

/** The runtime JSON text for the `merge` command. */
export function mergeText(locale, inputs) {
  if (!inputs.length) throw new Error('merge needs at least one input file');
  const tables = inputs.map((f) => {
    const text = readFileSync(f, 'utf8');
    if (f.endsWith('.json')) {
      const json = JSON.parse(text);
      if (json.locale !== locale) throw new Error(`${f} is for ${json.locale}, not ${locale}`);
      return Object.fromEntries(messagesFromJson(json).map((m) => [m.id, m.source]));
    }
    const messages = parseXliff(text);
    return locale === PSEUDO_LOCALE ? pseudoTranslations(messages) : targetTranslations(messages);
  });
  return runtimeJson(locale, mergeTranslations(tables));
}

function emit(file, content, check) {
  if (check) {
    const now = existsSync(file) ? readFileSync(file, 'utf8').replace(/\r\n/g, '\n') : null;
    if (now !== content) throw new Error(`${file} is out of date: regenerate it (npm run i18n) and commit the result`);
    return `${file} is up to date`;
  }
  writeFileSync(file, content);
  return `wrote ${file}`;
}

export function main(argv) {
  const [command, ...rest] = argv;
  const { flags, positional } = parseArgs(rest);
  const out = flags.get('out')?.[0];
  const check = flags.has('check');
  if (command === 'catalog') {
    if (!out) throw new Error('catalog needs --out <file.xlf>');
    return emit(out, catalogText(flags), check);
  }
  if (command === 'merge') {
    const locale = flags.get('locale')?.[0];
    if (!locale || !out) throw new Error('merge needs --locale <tag> and --out <file.json>');
    return emit(out, mergeText(locale, positional), check);
  }
  if (command === 'build') {
    const config = loadConfig(resolve(flags.get('config')?.[0] ?? 'paddock-i18n.json'));
    return runBuild(config, { check, extract: !flags.has('no-extract'), run: main }).join('\n');
  }
  throw new Error('usage: paddock-i18n build [--config f] [--no-extract] | paddock-i18n catalog --out <file.xlf> ... | paddock-i18n merge --locale <tag> --out <file.json> <input>...');
}

/** Run as the bin (also through npm's symlink in node_modules/.bin), not imported by the tests. */
function isMain() {
  try {
    return !!process.argv[1] && realpathSync(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));
  } catch {
    return false;
  }
}

if (isMain()) {
  try {
    console.log(`paddock-i18n: ${main(process.argv.slice(2))}`);
  } catch (e) {
    console.error(`paddock-i18n: ${e instanceof Error ? e.message : String(e)}`);
    process.exit(1);
  }
}
