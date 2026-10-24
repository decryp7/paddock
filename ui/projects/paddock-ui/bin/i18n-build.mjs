// `paddock-i18n build`: one declared build of an app's translation files (Paddock plan X48; generalised from Paddock's
// own scripts/build-i18n.mjs and Debrief's scripts/i18n/build-locales.mjs). The app writes `paddock-i18n.json` once and
// runs `paddock-i18n build` (regenerate) or `paddock-i18n build --check` (CI and the conformance check: writes nothing,
// fails on drift) instead of keeping its own script.
//
//   {
//     "extract":  { "dist": "dist/app", "sources": "**/*.js", "locale": "en-US" },       // optional: `$localize` from a build
//     "catalogs": [ { "out": "src/locale/messages.xlf", "prefix": "pw.", "src": ["src"], "extracted": true,
//                     "json": [], "manifest": [], "xlf": [] } ],
//     "locales":  [ { "locale": "en-XA", "out": "src/locale/messages.en-XA.json",
//                     "inputs": ["node_modules/@decryp7/paddock-ui/i18n/messages.pd.en-XA.json", "src/locale/messages.xlf"] } ],
//     "translated": [ { "dir": "src/locale", "with": ["node_modules/@decryp7/paddock-ui/i18n/messages.pd.{locale}.xlf"] } ]
//   }
//
// Every path is relative to the config file. `extracted: true` takes the catalog's messages from the `extract` run (the
// built output, so every lazy chunk counts); `--no-extract` skips that run and the catalog is read from disk as committed
// (the conformance check runs without a build). `translated`: each `messages.<tag>.xlf` in `dir` becomes
// `messages.<tag>.json` (its <target>s, plus the `with` files that exist for that tag), as Debrief's build-locales did.
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join, resolve } from 'node:path';

const LOCALE_TAG = /^[A-Za-z]{2,3}(?:-[A-Za-z0-9]+)*$/;

function strings(value, where) {
  if (value === undefined) return [];
  if (!Array.isArray(value) || value.some((s) => typeof s !== 'string' || !s)) throw new Error(`${where} must be a list of paths`);
  return value;
}

function text(value, where) {
  if (typeof value !== 'string' || !value) throw new Error(`${where} must be a non-empty string`);
  return value;
}

/**
 * Parses the untrusted `paddock-i18n.json` into the typed plan (parse, don't validate): every path made absolute against
 * `baseDir`, every wrong shape named in the error.
 */
export function parseConfig(json, baseDir) {
  if (typeof json !== 'object' || json === null || Array.isArray(json)) throw new Error('paddock-i18n.json must be an object');
  const at = (p) => resolve(baseDir, p);
  let extract = null;
  if (json.extract !== undefined) {
    const e = json.extract;
    if (typeof e !== 'object' || e === null) throw new Error('"extract" must be an object');
    extract = { dist: at(text(e.dist, '"extract.dist"')), sources: text(e.sources ?? '**/*.js', '"extract.sources"'), locale: text(e.locale ?? 'en-US', '"extract.locale"') };
  }
  if (!Array.isArray(json.catalogs) || !json.catalogs.length) throw new Error('"catalogs" must list at least one catalog');
  const catalogs = json.catalogs.map((c, i) => {
    const where = `catalogs[${i}]`;
    if (typeof c !== 'object' || c === null) throw new Error(`${where} must be an object`);
    if (c.extracted && !extract) throw new Error(`${where} takes the extracted messages but there is no "extract" section`);
    return {
      out: at(text(c.out, `${where}.out`)),
      prefix: c.prefix === undefined ? '' : text(c.prefix, `${where}.prefix`),
      src: strings(c.src, `${where}.src`).map(at),
      json: strings(c.json, `${where}.json`).map(at),
      manifest: strings(c.manifest, `${where}.manifest`).map(at),
      xlf: strings(c.xlf, `${where}.xlf`).map(at),
      extracted: c.extracted === true,
    };
  });
  const locales = (json.locales ?? []).map((l, i) => {
    const where = `locales[${i}]`;
    if (typeof l !== 'object' || l === null) throw new Error(`${where} must be an object`);
    const locale = text(l.locale, `${where}.locale`);
    if (!LOCALE_TAG.test(locale)) throw new Error(`${where}.locale ${JSON.stringify(locale)} is not a BCP 47 tag`);
    const inputs = strings(l.inputs, `${where}.inputs`).map(at);
    if (!inputs.length) throw new Error(`${where} needs "inputs"`);
    return { locale, out: at(text(l.out, `${where}.out`)), inputs };
  });
  const translated = (json.translated ?? []).map((t, i) => {
    const where = `translated[${i}]`;
    if (typeof t !== 'object' || t === null) throw new Error(`${where} must be an object`);
    return { dir: at(text(t.dir, `${where}.dir`)), prefix: t.prefix === undefined ? 'messages' : text(t.prefix, `${where}.prefix`), with: strings(t.with, `${where}.with`) };
  });
  return { extract, catalogs, locales, translated, baseDir };
}

/** Reads and parses a config file; a missing or broken file is an error that names it. */
export function loadConfig(file) {
  let raw;
  try {
    raw = JSON.parse(readFileSync(file, 'utf8'));
  } catch (e) {
    throw new Error(`${file}: ${e.code === 'ENOENT' ? 'not found (run from the app folder or pass --config)' : e.message}`);
  }
  return parseConfig(raw, dirname(file));
}

/** The translated catalogs of a folder: `messages.<tag>.xlf` -> `{ locale, xlf }`, sorted by tag, the pseudo-locale excluded. */
export function translatedCatalogs(dir, prefix = 'messages') {
  if (!existsSync(dir)) return [];
  const re = new RegExp(`^${prefix.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\.([A-Za-z]{2,3}(?:-[A-Za-z0-9]+)*)\\.xlf$`);
  return readdirSync(dir)
    .map((name) => ({ name, m: re.exec(name) }))
    .filter((x) => x.m && x.m[1] !== 'en-XA')
    .map((x) => ({ locale: x.m[1], xlf: join(dir, x.name) }))
    .sort((a, b) => (a.locale < b.locale ? -1 : 1));
}

/**
 * The steps of a build, in order, as `paddock-i18n` argument lists: each catalog, each declared locale, each discovered
 * translation. `extractedFile` is the JSON the extraction wrote (null when it was skipped).
 */
export function planSteps(config, { extractedFile = null, extract = true } = {}) {
  const steps = [];
  for (const c of config.catalogs) {
    if (c.extracted && !extract) {
      if (!existsSync(c.out)) throw new Error(`${c.out} is missing: run \`paddock-i18n build\` after a build first`);
      continue;
    }
    const args = ['catalog', '--out', c.out];
    if (c.prefix) args.push('--prefix', c.prefix);
    for (const [flag, list] of [['--src', c.src], ['--json', c.json], ['--manifest', c.manifest], ['--xlf', c.xlf]]) for (const f of list) args.push(flag, f);
    if (c.extracted && extractedFile) args.push('--json', extractedFile);
    steps.push(args);
  }
  for (const l of config.locales) steps.push(['merge', '--locale', l.locale, '--out', l.out, ...l.inputs]);
  for (const t of config.translated) {
    for (const { locale, xlf } of translatedCatalogs(t.dir, t.prefix)) {
      const extra = t.with.map((w) => resolve(config.baseDir, w.replaceAll('{locale}', locale))).filter((f) => existsSync(f));
      steps.push(['merge', '--locale', locale, '--out', join(t.dir, `${t.prefix}.${locale}.json`), xlf, ...extra]);
    }
  }
  return steps;
}

/** Runs Angular's `localize-extract` over the built output into `out` (JSON, source locale). */
export function localizeExtract(extract, out, baseDir) {
  if (!existsSync(extract.dist)) throw new Error(`${extract.dist} is missing: build the app first (or pass --no-extract to use the committed catalogs)`);
  let cli;
  try {
    const pkg = createRequire(join(baseDir, 'package.json')).resolve('@angular/localize/package.json');
    cli = join(dirname(pkg), 'tools', 'bundles', 'src', 'extract', 'cli.js');
  } catch {
    throw new Error('@angular/localize is not installed: `localize-extract` is needed to read the built messages');
  }
  mkdirSync(dirname(out), { recursive: true });
  execFileSync(
    process.execPath,
    [cli, '-r', extract.dist, '-s', extract.sources, '-f', 'json', '-o', out, '--useSourceMaps=false', '-l', extract.locale, '-d', 'error', '--loglevel', 'warn'],
    { stdio: 'inherit', timeout: 5 * 60_000 },
  );
}

/**
 * Runs the build of `config`. `run(args)` executes one `paddock-i18n` command and returns its message; `extractor` writes
 * the extraction (both injectable for tests). Returns the messages, one per step.
 */
export function runBuild(config, { check = false, extract = true, run, extractor = localizeExtract, workDir = join(config.baseDir, '.angular', 'i18n') }) {
  const wantsExtraction = extract && config.extract && config.catalogs.some((c) => c.extracted);
  let extractedFile = null;
  if (wantsExtraction) {
    mkdirSync(workDir, { recursive: true });
    extractedFile = join(workDir, 'extracted.json');
    extractor(config.extract, extractedFile, config.baseDir);
  }
  const steps = planSteps(config, { extractedFile, extract: !!wantsExtraction || !config.catalogs.some((c) => c.extracted) });
  const messages = [];
  for (const args of steps) {
    if (!check) mkdirSync(dirname(args[args.indexOf('--out') + 1]), { recursive: true });
    messages.push(run(check ? [...args, '--check'] : args));
  }
  return messages;
}
