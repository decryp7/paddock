// Tests of `paddock-i18n build` (node --test): the config parser, the plan, and whole builds in a temporary app folder
// with a fake extractor (no Angular build, no network).
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, it } from 'node:test';
import { parseXliff, writeXliff } from './i18n-messages.mjs';
import { loadConfig, parseConfig, planSteps, runBuild, translatedCatalogs } from './i18n-build.mjs';
import { main } from './paddock-i18n.mjs';

const root = mkdtempSync(join(tmpdir(), 'paddock-i18n-build-'));
after(() => rmSync(root, { recursive: true, force: true }));
let n = 0;
/** A fresh app folder. */
function app() {
  const dir = join(root, `app${n++}`);
  mkdirSync(dir, { recursive: true });
  return dir;
}
const put = (dir, name, content) => {
  const p = join(dir, name);
  mkdirSync(dirname(p), { recursive: true });
  writeFileSync(p, content);
  return p;
};
const read = (dir, name) => readFileSync(join(dir, name), 'utf8');
const CONFIG = {
  extract: { dist: 'dist/app' },
  catalogs: [{ out: 'src/locale/messages.xlf', prefix: 'ap.', src: ['src'], extracted: true }],
  locales: [{ locale: 'en-XA', out: 'src/locale/messages.en-XA.json', inputs: ['src/locale/messages.xlf'] }],
  translated: [{ dir: 'src/locale', with: ['shell/messages.pd.{locale}.xlf'] }],
};

/** An app with sources, a built output folder, a config and a fake extraction that reports `translations`. */
function setup(translations = { 'ap.title': 'Title' }, config = CONFIG) {
  const dir = app();
  put(dir, 'src/a.ts', "export const A = msg('ap.menu', 'Menu');\n");
  put(dir, 'dist/app/main.js', '');
  put(dir, 'paddock-i18n.json', JSON.stringify(config));
  const extractor = (_extract, out) => writeFileSync(out, JSON.stringify({ locale: 'en-US', translations }));
  return { dir, extractor, config: () => loadConfig(join(dir, 'paddock-i18n.json')) };
}

describe('paddock-i18n.json', () => {
  it('resolves paths against the config folder and defaults the optional parts', () => {
    const c = parseConfig(CONFIG, '/app');
    assert.equal(c.extract.dist, '/app/dist/app');
    assert.equal(c.extract.locale, 'en-US');
    assert.deepEqual(c.catalogs[0].src, ['/app/src']);
    assert.equal(c.catalogs[0].extracted, true);
    assert.equal(c.translated[0].prefix, 'messages');
  });

  it('names what is wrong, once, at the edge', () => {
    assert.throws(() => parseConfig([], '/a'), /must be an object/);
    assert.throws(() => parseConfig({ catalogs: [] }, '/a'), /at least one catalog/);
    assert.throws(() => parseConfig({ catalogs: [{ out: 'x', extracted: true }] }, '/a'), /no "extract" section/);
    assert.throws(() => parseConfig({ catalogs: [{ out: '' }] }, '/a'), /catalogs\[0\]\.out must be a non-empty string/);
    assert.throws(() => parseConfig({ catalogs: [{ out: 'x', src: 'src' }] }, '/a'), /src must be a list of paths/);
    assert.throws(() => parseConfig({ catalogs: [{ out: 'x' }], locales: [{ locale: 'not a tag', out: 'o', inputs: ['i'] }] }, '/a'), /not a BCP 47 tag/);
    assert.throws(() => parseConfig({ catalogs: [{ out: 'x' }], locales: [{ locale: 'de', out: 'o' }] }, '/a'), /needs "inputs"/);
    assert.throws(() => parseConfig({ extract: 3, catalogs: [{ out: 'x' }] }, '/a'), /"extract" must be an object/);
  });

  it('a missing or broken file is an error naming the file', () => {
    const dir = app();
    assert.throws(() => loadConfig(join(dir, 'paddock-i18n.json')), /not found/);
    put(dir, 'paddock-i18n.json', '{ nope');
    assert.throws(() => loadConfig(join(dir, 'paddock-i18n.json')), /paddock-i18n\.json:/);
  });
});

describe('plan', () => {
  it('finds translated catalogs by tag, never the pseudo-locale', () => {
    const dir = app();
    for (const f of ['messages.xlf', 'messages.de.xlf', 'messages.pt-BR.xlf', 'messages.en-XA.xlf', 'messages.de.json', 'other.fr.xlf']) put(dir, f, '');
    assert.deepEqual(translatedCatalogs(dir).map((t) => t.locale), ['de', 'pt-BR']);
    assert.deepEqual(translatedCatalogs(join(dir, 'missing')), []);
  });

  it('orders catalogs, then locales, then translations; a with-file is used only where it exists', () => {
    const { dir, config } = setup();
    put(dir, 'src/locale/messages.de.xlf', '');
    put(dir, 'src/locale/messages.fr.xlf', '');
    put(dir, 'shell/messages.pd.de.xlf', '');
    const steps = planSteps(config(), { extractedFile: '/x/e.json' });
    assert.deepEqual(steps.map((s) => [s[0], s[s.indexOf('--locale') + 1] ?? 'catalog']), [['catalog', 'catalog'], ['merge', 'en-XA'], ['merge', 'de'], ['merge', 'fr']]);
    assert.deepEqual(steps[0].slice(-2), ['--json', '/x/e.json']);
    assert.equal(steps[2].filter((a) => a.endsWith('.xlf')).length, 2);
    assert.equal(steps[3].filter((a) => a.endsWith('.xlf')).length, 1);
  });
});

describe('build', () => {
  it('writes the catalog from the extraction and the sources, then the pseudo-locale; --check agrees and then catches drift', () => {
    const { dir, extractor, config } = setup();
    const run = (args) => main(args);
    const out = runBuild(config(), { run, extractor });
    assert.equal(out.length, 2);
    assert.deepEqual(parseXliff(read(dir, 'src/locale/messages.xlf')).map((m) => m.id), ['ap.menu', 'ap.title']);
    assert.ok(JSON.parse(read(dir, 'src/locale/messages.en-XA.json')).translations['ap.title'].startsWith('⟦'));
    assert.deepEqual(runBuild(config(), { run, extractor, check: true }).map((m) => /up to date/.test(m)), [true, true]);
    // A new string in the build makes the committed catalog stale.
    const stale = setup({ 'ap.title': 'Title', 'ap.more': 'More' });
    runBuild(stale.config(), { run, extractor });
    const newer = (_e, o) => writeFileSync(o, JSON.stringify({ locale: 'en-US', translations: { 'ap.title': 'Title', 'ap.more': 'More', 'ap.newest': 'Newest' } }));
    assert.throws(() => runBuild(stale.config(), { run, extractor: newer, check: true }), /messages\.xlf is out of date/);
  });

  it('a check writes nothing, not even folders', () => {
    const { dir, extractor, config } = setup();
    assert.throws(() => runBuild(config(), { run: main, extractor, check: true }), /out of date/);
    assert.throws(() => readFileSync(join(dir, 'src/locale/messages.xlf')), /ENOENT/);
  });

  it('translated catalogs become runtime JSON with the shell’s translations merged in; a stale JSON fails the check', () => {
    const { dir, extractor, config } = setup();
    put(dir, 'src/locale/messages.de.xlf', writeXliff([{ id: 'ap.title', source: 'Title' }]).replace('<source>Title</source>', '<source>Title</source>\n<target>Titel</target>'));
    put(dir, 'shell/messages.pd.de.xlf', writeXliff([{ id: 'pd.close', source: 'Close' }]).replace('<source>Close</source>', '<source>Close</source>\n<target>Schließen</target>'));
    runBuild(config(), { run: main, extractor });
    assert.deepEqual(JSON.parse(read(dir, 'src/locale/messages.de.json')), { locale: 'de', translations: { 'ap.title': 'Titel', 'pd.close': 'Schließen' } });
    runBuild(config(), { run: main, extractor, check: true });
    writeFileSync(join(dir, 'src/locale/messages.de.json'), '{}\n');
    assert.throws(() => runBuild(config(), { run: main, extractor, check: true }), /messages\.de\.json is out of date/);
  });

  it('--no-extract reads the committed catalog (no build needed) and still checks what follows from it', () => {
    const { dir, extractor, config } = setup();
    runBuild(config(), { run: main, extractor });
    rmSync(join(dir, 'dist'), { recursive: true });
    const boom = () => assert.fail('must not extract');
    assert.deepEqual(runBuild(config(), { run: main, extractor: boom, extract: false, check: true }).length, 1);
    rmSync(join(dir, 'src/locale/messages.xlf'));
    assert.throws(() => runBuild(config(), { run: main, extractor: boom, extract: false, check: true }), /messages\.xlf is missing/);
  });

  it('a missing build output says to build first', () => {
    const { dir, config } = setup();
    rmSync(join(dir, 'dist'), { recursive: true });
    assert.throws(() => runBuild(config(), { run: main }), /build the app first/);
  });

  it('is a command of the bin: build --config, --check and --no-extract', () => {
    const { dir } = setup();
    const cfg = join(dir, 'paddock-i18n.json');
    // No extraction possible here (the fake dist has no messages): a config without extracted catalogs builds from sources.
    put(dir, 'plain.json', JSON.stringify({ catalogs: [{ out: 'plain/messages.xlf', prefix: 'ap.', src: ['src'] }], locales: [{ locale: 'en-XA', out: 'plain/p.json', inputs: ['plain/messages.xlf'] }] }));
    assert.match(main(['build', '--config', join(dir, 'plain.json')]), /wrote .*messages\.xlf\nwrote .*p\.json/);
    assert.match(main(['build', '--config', join(dir, 'plain.json'), '--check']), /up to date/);
    assert.throws(() => main(['build', '--config', join(dir, 'nope.json')]), /not found/);
    assert.ok(cfg);
  });
});
