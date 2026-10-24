// Tests of paddock-i18n (node --test): the pure helpers and the two commands on files in a temporary folder.
import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';
import {
  buildCatalog,
  mergeTranslations,
  messagesFromJson,
  messagesFromManifest,
  parseMessage,
  parseXliff,
  placeholders,
  pseudoMessage,
  pseudoText,
  scanMsgCalls,
  serializeMessage,
  targetTranslations,
  writeXliff,
} from './i18n-messages.mjs';
import { main, parseArgs } from './paddock-i18n.mjs';

const dir = mkdtempSync(join(tmpdir(), 'paddock-i18n-'));
after(() => rmSync(dir, { recursive: true, force: true }));
const file = (name, content) => {
  const p = join(dir, name);
  mkdirSync(join(p, '..'), { recursive: true });
  writeFileSync(p, content);
  return p;
};

describe('msg() scanner', () => {
  it('finds literal calls on one or several lines, with either quote and escapes', () => {
    const src = `
      const a = msg('pd.menu.file', 'File');
      const b = msg(
        "pd.startup.terminated",
        "The window's content stopped.",
      );
      const c = msg('pd.x.quote', 'It\\'s \\u00e9');`;
    assert.deepEqual(scanMsgCalls(src), {
      messages: [
        { id: 'pd.menu.file', source: 'File' },
        { id: 'pd.startup.terminated', source: "The window's content stopped." },
        { id: 'pd.x.quote', source: "It's é" },
      ],
      dynamic: [],
    });
  });

  it('reports calls whose text is not a literal, and skips the definition and mentions in comments and strings', () => {
    const src = [
      'export function msg(id: string, text: string) {}',
      '// use msg(id, text) here',
      " * msg('pw.models.refresh', 'Refresh models')",
      "const lint = `title has no message id (use msg('<ns>.<name>', 'Text'))`;",
      'const t = msg(`pd.${name}`, label);',
    ].join('\n');
    assert.deepEqual(scanMsgCalls(src).dynamic, [5]);
  });
});

describe('catalog', () => {
  it('joins lists, keeps one unit per id, filters by prefix and refuses one id with two texts', () => {
    const c = buildCatalog([[{ id: 'pd.b', source: 'B' }, { id: 'sd.x', source: 'X' }], [{ id: 'pd.a', source: 'A' }, { id: 'pd.b', source: 'B', description: 'why' }]], 'pd.');
    assert.deepEqual(c, [{ id: 'pd.a', source: 'A' }, { id: 'pd.b', source: 'B', description: 'why' }]);
    assert.throws(() => buildCatalog([[{ id: 'pd.a', source: 'A' }], [{ id: 'pd.a', source: 'a' }]]), /pd\.a has two texts/);
  });

  it('writes XLIFF that reads back to the same messages, placeholders and ICU included', () => {
    const messages = [
      { id: 'pd.jobs.status.running', source: '{$count} running' },
      { id: 'pd.amp', source: 'Fish & <chips> "now"', description: 'a & b' },
      { id: 'pd.icu', source: '{VAR_PLURAL, plural, =1 {one file} other {{INTERPOLATION} files}}' },
    ];
    const xml = writeXliff(messages);
    assert.match(xml, /<x id="count"\/> running/);
    assert.doesNotMatch(xml, /sourcefile/);
    const back = parseXliff(xml);
    assert.deepEqual(back.map((m) => [m.id, m.source]).sort(), messages.map((m) => [m.id, m.source]).sort());
    assert.equal(back.find((m) => m.id === 'pd.amp').description, 'a & b');
  });

  it('reads extracted JSON and manifests and refuses anything else', () => {
    assert.deepEqual(messagesFromJson({ locale: 'en-US', translations: { 'pd.a': 'A' } }), [{ id: 'pd.a', source: 'A' }]);
    assert.throws(() => messagesFromJson({}), /no "translations"/);
    assert.throws(() => messagesFromJson({ translations: { a: 1 } }), /not text/);
    assert.deepEqual(messagesFromManifest({ messages: { 'sd.say-hello': 'Say hello' } }), [{ id: 'sd.say-hello', source: 'Say hello' }]);
    assert.throws(() => messagesFromManifest({ version: 1 }), /no "messages"/);
  });
});

describe('pseudo-locale and translations', () => {
  it('accents, lengthens and brackets text but keeps placeholders and ICU syntax', () => {
    assert.equal(pseudoText('Jobs'), 'Ĵööƀš');
    assert.equal(pseudoMessage('{$count} running'), '⟦{$count} ŕûûññîîñĝ⟧');
    const icu = pseudoMessage('{VAR_PLURAL, plural, =1 {one} other {{INTERPOLATION} many}}');
    assert.equal(icu, '{VAR_PLURAL, plural, =1 {⟦ööñéé⟧} other {⟦{INTERPOLATION} ɱááñý⟧}}');
    assert.deepEqual(placeholders(parseMessage(icu)), ['INTERPOLATION']);
    assert.equal(serializeMessage(parseMessage('a {b} c')), 'a {b} c');
  });

  it('takes the targets of a translated catalog and refuses a target with other placeholders', () => {
    assert.deepEqual(targetTranslations([{ id: 'a', source: '{$n} x', target: '{$n} y' }, { id: 'b', source: 'B' }]), { a: '{$n} y' });
    assert.throws(() => targetTranslations([{ id: 'a', source: '{$n} x', target: 'y' }]), /placeholders/);
  });

  it('merges the shell and the app, refusing an id translated two ways', () => {
    assert.deepEqual(mergeTranslations([{ 'pd.a': 'A' }, { 'sd.b': 'B', 'pd.a': 'A' }]), { 'pd.a': 'A', 'sd.b': 'B' });
    assert.throws(() => mergeTranslations([{ 'pd.a': 'A' }, { 'pd.a': 'Z' }]), /translated twice/);
  });
});

describe('paddock-i18n commands', () => {
  it('parses repeated flags and positional inputs', () => {
    const { flags, positional } = parseArgs(['--src', 'a', '--src', 'b', 'in.json', '--check']);
    assert.deepEqual(flags.get('src'), ['a', 'b']);
    assert.deepEqual(positional, ['in.json']);
    assert.ok(flags.has('check'));
    assert.throws(() => parseArgs(['--out']), /needs a value/);
  });

  it('catalog: $localize JSON + msg() sources -> XLIFF; --check passes on the result and fails on drift', () => {
    const src = join(dir, 'src');
    file('src/app/a.contributions.ts', "export const A = msg('pd.menu.file', 'File');\nexport const B = msg('sd.other', 'Other');\n");
    file('src/app/a.spec.ts', "msg('pd.spec.only', 'Spec');\n");
    const extracted = file('extracted.json', JSON.stringify({ locale: 'en-US', translations: { 'pd.jobs.list': 'Jobs', 'sd.x': 'X' } }));
    const out = join(dir, 'messages.pd.xlf');
    assert.match(main(['catalog', '--out', out, '--json', extracted, '--src', src, '--prefix', 'pd.']), /wrote/);
    const ids = parseXliff(readFileSync(out, 'utf8')).map((m) => m.id);
    assert.deepEqual(ids, ['pd.jobs.list', 'pd.menu.file']);
    assert.match(main(['catalog', '--out', out, '--json', extracted, '--src', src, '--prefix', 'pd.', '--check']), /up to date/);
    file('src/app/b.ts', "msg('pd.menu.edit', 'Edit');\n");
    assert.throws(() => main(['catalog', '--out', out, '--json', extracted, '--src', src, '--prefix', 'pd.', '--check']), /out of date/);
  });

  it('catalog: a manifest is a source too; an empty catalog and a dynamic msg() fail', () => {
    const manifest = file('m/paddock.manifest.json', JSON.stringify({ version: 1, messages: { 'sd.say-hello': 'Say hello' } }));
    const out = join(dir, 'sd.xlf');
    main(['catalog', '--out', out, '--manifest', manifest]);
    assert.equal(parseXliff(readFileSync(out, 'utf8'))[0].source, 'Say hello');
    assert.throws(() => main(['catalog', '--out', out, '--manifest', manifest, '--prefix', 'zz.']), /would be empty/);
    const bad = join(dir, 'bad');
    file('bad/x.ts', 'msg(id, text);\n');
    assert.throws(() => main(['catalog', '--out', out, '--src', bad]), /x\.ts:1: msg\(\) needs two string literals/);
  });

  it('merge: the pseudo-locale from catalogs plus the package JSON; the locale of a JSON input must match', () => {
    const xlf = file('app.xlf', writeXliff([{ id: 'sd.say-hello', source: 'Say hello' }]));
    const pkg = file('messages.pd.en-XA.json', JSON.stringify({ locale: 'en-XA', translations: { 'pd.menu.file': '⟦Ƒîîļé⟧' } }));
    const out = join(dir, 'messages.en-XA.json');
    main(['merge', '--locale', 'en-XA', '--out', out, pkg, xlf]);
    assert.deepEqual(JSON.parse(readFileSync(out, 'utf8')), { locale: 'en-XA', translations: { 'pd.menu.file': '⟦Ƒîîļé⟧', 'sd.say-hello': '⟦Šááý ĥééļļöö⟧' } });
    assert.match(main(['merge', '--locale', 'en-XA', '--out', out, pkg, xlf, '--check']), /up to date/);
    assert.throws(() => main(['merge', '--locale', 'de-DE', '--out', out, pkg]), /is for en-XA, not de-DE/);
    assert.throws(() => main(['merge', '--locale', 'de-DE', '--out', out]), /at least one input/);
  });

  it('a real locale takes the <target>s of a translated catalog', () => {
    const xlf = file(
      'de.xlf',
      writeXliff([{ id: 'pd.a', source: 'A' }]).replace('<source>A</source>', '<source>A</source>\n        <target>Ä</target>'),
    );
    const out = join(dir, 'messages.de-DE.json');
    main(['merge', '--locale', 'de-DE', '--out', out, xlf]);
    assert.deepEqual(JSON.parse(readFileSync(out, 'utf8')).translations, { 'pd.a': 'Ä' });
  });

  it('refuses unknown commands and missing flags with the usage', () => {
    assert.throws(() => main(['translate']), /usage/);
    assert.throws(() => main(['catalog']), /needs --out/);
    assert.throws(() => main(['merge', '--out', 'x']), /needs --locale/);
  });
});
