// Pure helpers of `paddock-i18n` (Paddock ADR 0012 §6; ported from Debrief's scripts/i18n/locales.mjs): XLIFF 1.2 (the
// catalog translators edit) to and from the runtime JSON `loadTranslations` takes, the `msg(id, text)` scanner, the
// generated pseudo-locale and the merge of the shell's and the app's tables. No I/O here; see paddock-i18n.mjs.

const ENTITIES = { lt: '<', gt: '>', quot: '"', apos: "'", amp: '&' };

function unescapeXml(s) {
  return s.replace(/&(lt|gt|quot|apos|amp|#(\d+)|#x([0-9a-fA-F]+));/g, (_, name, dec, hex) =>
    dec ? String.fromCodePoint(Number(dec)) : hex ? String.fromCodePoint(parseInt(hex, 16)) : ENTITIES[name],
  );
}

/** XLIFF message content -> runtime text: `<x id="PH"/>` becomes `{$PH}`, entities are decoded. */
export function xliffContentToText(content) {
  return unescapeXml(content.replace(/<x\s+id="([^"]+)"[^>]*\/>/g, (_, id) => `{$${id}}`));
}

/** XLIFF content in the exact runtime form (placeholders inside ICU cases are written without the `$`). */
function runtimeText(content) {
  return serializeMessage(parseMessage(xliffContentToText(content)));
}

function noteOf(body, from) {
  const m = new RegExp(`<note[^>]*from="${from}"[^>]*>([\\s\\S]*?)</note>`).exec(body);
  return m ? unescapeXml(m[1]) : undefined;
}

/**
 * Parses an XLIFF 1.2 file into messages: `{ id, source, target?, meaning?, description? }` in runtime text form.
 * Throws on a file without trans-units or with a unit that has no source (a broken file must not pass silently).
 */
export function parseXliff(xml) {
  const units = [...xml.matchAll(/<trans-unit\s+id="([^"]+)"[^>]*>([\s\S]*?)<\/trans-unit>/g)];
  return units.map(([, id, body]) => {
    const source = /<source>([\s\S]*?)<\/source>/.exec(body);
    if (!source) throw new Error(`trans-unit ${id} has no <source>`);
    const target = /<target(?:\s[^>]*)?>([\s\S]*?)<\/target>/.exec(body);
    const msg = { id: unescapeXml(id), source: runtimeText(source[1]) };
    if (target) msg.target = runtimeText(target[1]);
    const meaning = noteOf(body, 'meaning');
    const description = noteOf(body, 'description');
    if (meaning !== undefined) msg.meaning = meaning;
    if (description !== undefined) msg.description = description;
    return msg;
  });
}

/** What must stay in sync between the committed source file and a fresh extraction (locations are ignored). */
export function normalizeMessages(messages) {
  return messages
    .map(({ id, source, meaning, description }) => ({ id, source, meaning: meaning ?? null, description: description ?? null }))
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

// ---- runtime message syntax: text, {$PLACEHOLDER} and ICU {VAR, plural|select, case {message} ...} ----

/** An ICU expression as Angular extracts it from a template: `{VAR_PLURAL, plural, ...}`, `{VAR_SELECT_1, select, ...}`. */
const ICU_START = /^\{VAR_(?:PLURAL|SELECT)(?:_\d+)?\s*,/;
/** A placeholder inside an ICU case: `{INTERPOLATION}`, `{START_TAG_B}` (no `$`). */
const ICU_PLACEHOLDER = /^\{[A-Z][A-Z0-9_]*\}/;

/**
 * Parses runtime message text into nodes: `{ text }`, `{ ph }` and `{ icu: { name, type, cases: [{ key, nodes }] } }`.
 * Any other brace is literal text (`$localize` messages from TypeScript have no ICU).
 */
export function parseMessage(s) {
  let i = 0;
  function nodes(inCase) {
    const out = [];
    let text = '';
    const flush = () => {
      if (text) out.push({ text });
      text = '';
    };
    while (i < s.length) {
      const c = s[i];
      if (c === '{' && s[i + 1] === '$') {
        const end = s.indexOf('}', i);
        if (end < 0) throw new Error(`unclosed placeholder in: ${s}`);
        flush();
        out.push({ ph: s.slice(i + 2, end) });
        i = end + 1;
      } else if (c === '{' && inCase && ICU_PLACEHOLDER.test(s.slice(i))) {
        const end = s.indexOf('}', i);
        flush();
        out.push({ ph: s.slice(i + 1, end) });
        i = end + 1;
      } else if (c === '{' && ICU_START.test(s.slice(i, i + 40))) {
        flush();
        out.push({ icu: icu() });
      } else if (c === '}' && inCase) {
        flush();
        i++;
        return out;
      } else {
        text += c;
        i++;
      }
    }
    if (inCase) throw new Error(`unclosed ICU case in: ${s}`);
    flush();
    return out;
  }
  function until(ch) {
    const end = s.indexOf(ch, i);
    if (end < 0) throw new Error(`expected "${ch}" in: ${s}`);
    const v = s.slice(i, end).trim();
    i = end + 1;
    return v;
  }
  function icu() {
    i++; // {
    const name = until(',');
    const type = until(',');
    const cases = [];
    for (;;) {
      while (/\s/.test(s[i] ?? '')) i++;
      if (s[i] === '}') {
        i++;
        return { name, type, cases };
      }
      if (i >= s.length) throw new Error(`unclosed ICU in: ${s}`);
      const key = until('{');
      cases.push({ key, nodes: nodes(true) });
    }
  }
  return nodes(false);
}

/**
 * Inverse of `parseMessage`. Placeholders are written `{$NAME}` in a message but `{NAME}` inside ICU cases, which is
 * how Angular's runtime (and `ng extract-i18n --format json`) expects them.
 */
export function serializeMessage(nodes, inIcu = false) {
  return nodes
    .map((n) => {
      if ('text' in n) return n.text;
      if ('ph' in n) return inIcu ? `{${n.ph}}` : `{$${n.ph}}`;
      const cases = n.icu.cases.map((c) => `${c.key} {${serializeMessage(c.nodes, true)}}`).join(' ');
      return `{${n.icu.name}, ${n.icu.type}, ${cases}}`;
    })
    .join('');
}

/** Placeholder names a message uses (incl. inside ICU cases), sorted, for comparing a translation with its source. */
export function placeholders(nodes, acc = new Set()) {
  for (const n of nodes) {
    if ('ph' in n) acc.add(n.ph);
    else if ('icu' in n) for (const c of n.icu.cases) placeholders(c.nodes, acc);
  }
  return [...acc].sort();
}

// ---- pseudo-locale ----

const ACCENTS = {
  a: 'á', b: 'ƀ', c: 'ç', d: 'ð', e: 'é', f: 'ƒ', g: 'ĝ', h: 'ĥ', i: 'î', j: 'ĵ', k: 'ķ', l: 'ļ', m: 'ɱ',
  n: 'ñ', o: 'ö', p: 'þ', q: 'ǫ', r: 'ŕ', s: 'š', t: 'ţ', u: 'û', v: 'ṽ', w: 'ŵ', x: 'ẋ', y: 'ý', z: 'ž',
  A: 'Å', B: 'Ɓ', C: 'Ç', D: 'Ð', E: 'É', F: 'Ƒ', G: 'Ĝ', H: 'Ĥ', I: 'Î', J: 'Ĵ', K: 'Ķ', L: 'Ļ', M: 'Ṁ',
  N: 'Ñ', O: 'Ö', P: 'Þ', Q: 'Ǫ', R: 'Ŕ', S: 'Š', T: 'Ţ', U: 'Û', V: 'Ṽ', W: 'Ŵ', X: 'Ẋ', Y: 'Ý', Z: 'Ž',
};
const VOWELS = new Set('aeiouAEIOU');

/**
 * Accents every ASCII letter and doubles three of every four vowels (about 30% longer, spread through the words so
 * wrapping stays realistic).
 */
export function pseudoText(text) {
  let vowel = 0;
  let out = '';
  for (const ch of text) {
    const a = ACCENTS[ch] ?? ch;
    out += a;
    if (VOWELS.has(ch) && vowel++ % 4 !== 3) out += a;
  }
  return out;
}

function pseudoNodes(nodes) {
  return nodes.map((n) => {
    if ('text' in n) return { text: pseudoText(n.text) };
    if ('icu' in n) return { icu: { ...n.icu, cases: n.icu.cases.map((c) => ({ key: c.key, nodes: bracket(pseudoNodes(c.nodes)) })) } };
    return n;
  });
}

function bracket(nodes) {
  return [{ text: '⟦' }, ...nodes, { text: '⟧' }];
}

/**
 * The pseudo translation of a source message: accented, longer and bracketed (⟦…⟧) so truncation and unmarked
 * strings stand out. Placeholders and ICU syntax are kept; a message that is a single ICU is bracketed per case.
 */
export function pseudoMessage(source) {
  const nodes = parseMessage(source);
  const pseudo = pseudoNodes(nodes);
  const singleIcu = nodes.length === 1 && 'icu' in nodes[0];
  return serializeMessage(singleIcu ? pseudo : bracket(pseudo));
}

/** Runtime translations of the pseudo-locale for every source message. */
export function pseudoTranslations(messages) {
  return Object.fromEntries(messages.map((m) => [m.id, pseudoMessage(m.source)]));
}

/**
 * Runtime translations from a translated XLIFF file. Units without a target are left out (they fall back to English
 * at run time); a target whose placeholders differ from the source is an error.
 */
export function targetTranslations(messages) {
  const out = {};
  for (const m of messages) {
    if (m.target === undefined || m.target.trim() === '') continue;
    const want = placeholders(parseMessage(m.source)).join(',');
    const got = placeholders(parseMessage(m.target)).join(',');
    if (want !== got) throw new Error(`${m.id}: the translation uses placeholders [${got}], the source [${want}]`);
    out[m.id] = m.target;
  }
  return out;
}

/** The JSON file `loadTranslations` is fed with (same shape as `ng extract-i18n --format json`). */
export function runtimeJson(locale, translations) {
  const sorted = Object.fromEntries(Object.entries(translations).sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0)));
  return JSON.stringify({ locale, translations: sorted }, null, 2) + '\n';
}

// ---- writing the catalog ----

function escapeXml(s) {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
}

/** Runtime text -> XLIFF content: placeholders become `<x id="PH"/>`, ICU syntax stays as text. */
function xliffContent(source) {
  const write = (nodes) =>
    nodes
      .map((n) => {
        if ('text' in n) return escapeXml(n.text);
        if ('ph' in n) return `<x id="${escapeXml(n.ph)}"/>`;
        return `{${n.icu.name}, ${n.icu.type}, ${n.icu.cases.map((c) => `${c.key} {${write(c.nodes)}}`).join(' ')}}`;
      })
      .join('');
  return write(parseMessage(source));
}

/**
 * The XLIFF 1.2 catalog of `messages` (`{ id, source, description?, meaning? }`), sorted by id and without source
 * locations, so a regenerated catalog only changes where a message changed.
 */
export function writeXliff(messages, sourceLocale = 'en-US') {
  const units = [...messages]
    .sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0))
    .map((m) => {
      const notes = [
        m.description !== undefined ? `\n        <note priority="1" from="description">${escapeXml(m.description)}</note>` : '',
        m.meaning !== undefined ? `\n        <note priority="1" from="meaning">${escapeXml(m.meaning)}</note>` : '',
      ].join('');
      return `      <trans-unit id="${escapeXml(m.id)}" datatype="html">\n        <source>${xliffContent(m.source)}</source>${notes}\n      </trans-unit>\n`;
    })
    .join('');
  return (
    `<?xml version="1.0" encoding="UTF-8" ?>\n<xliff version="1.2" xmlns="urn:oasis:names:tc:xliff:document:1.2">\n` +
    `  <file source-language="${escapeXml(sourceLocale)}" datatype="plaintext" original="ng2.template">\n    <body>\n${units}    </body>\n  </file>\n</xliff>\n`
  );
}

// ---- msg(id, text) calls ----

/** A single-quoted or double-quoted JavaScript string literal (no template literals: their text could be computed). */
const LITERAL = String.raw`'(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*"`;
const MSG_CALL = new RegExp(String.raw`\bmsg\(\s*(${LITERAL})\s*,\s*(${LITERAL})\s*,?\s*\)`, 'g');
const ANY_MSG_CALL = /\bmsg\(/g;

function unquote(literal) {
  const body = literal.slice(1, -1);
  return body.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)/g, (_, e) => {
    if (e[0] === 'u') return String.fromCodePoint(parseInt(e.replace(/[u{}]/g, ''), 16));
    if (e[0] === 'x') return String.fromCharCode(parseInt(e.slice(1), 16));
    return { n: '\n', t: '\t', r: '\r', 0: '\0' }[e] ?? e;
  });
}

const TOKENS = /'(?:[^'\\\n]|\\.)*'|"(?:[^"\\\n]|\\.)*"|`(?:[^`\\]|\\.)*`|\/\*[\s\S]*?\*\/|\/\/[^\n]*/g;
/** `text` with every comment (and, with `strings`, every string literal) replaced by spaces; line breaks are kept. */
function blank(text, strings) {
  return text.replace(TOKENS, (t) => (t.startsWith('/') || strings ? t.replace(/[^\n]/g, ' ') : t));
}

/**
 * The `msg('id', 'Text')` calls in a TypeScript source (the contributions' messages, S2). Calls whose arguments are not
 * two string literals are returned in `dynamic` (with their line) so the caller can fail: their text cannot be
 * extracted. Comments, string literals (`use msg('<ns>.<name>', 'Text')` in a lint message) and the definition
 * `function msg(` are not calls.
 */
export function scanMsgCalls(source) {
  const code = blank(source, false);
  const found = [];
  const at = new Set();
  for (const m of code.matchAll(MSG_CALL)) {
    found.push({ id: unquote(m[1]), source: unquote(m[2]) });
    at.add(m.index);
  }
  const bare = blank(source, true);
  const dynamic = [];
  for (const m of bare.matchAll(ANY_MSG_CALL)) {
    if (at.has(m.index) || /function\s*$/.test(bare.slice(Math.max(0, m.index - 9), m.index))) continue;
    dynamic.push(bare.slice(0, m.index).split('\n').length);
  }
  return { messages: found, dynamic };
}

// ---- $localize`...` in TypeScript sources ----

const LOCALIZE = /\$localize`((?:[^`\\]|\\.)*)`/g;

/**
 * The `$localize` messages of a TypeScript source, for apps whose own `$localize` texts are not extracted from a build:
 * `$localize`:@@pw.models.count:${n}:count: models`` becomes `{ id: 'pw.models.count', source: '{$count} models' }`
 * (an expression without a `:name:` is `PH`, `PH_1`, ... as Angular names it). A message without an `@@id` is returned in
 * `unnamed` (with its line): the catalog keys by id, so it cannot be extracted.
 */
export function scanLocalizeCalls(source) {
  const code = blank(source, false);
  const messages = [];
  const unnamed = [];
  for (const m of code.matchAll(LOCALIZE)) {
    const raw = source.slice(m.index + '$localize`'.length, m.index + m[0].length - 1);
    const parts = raw.split(/\$\{[^}]*\}/);
    const head = /^:([^:]*):/.exec(parts[0] ?? '');
    const id = head ? /@@([^:]+)$/.exec(head[1])?.[1] : undefined;
    if (!id) {
      unnamed.push(source.slice(0, m.index).split('\n').length);
      continue;
    }
    let text = unquoteTemplate((parts[0] ?? '').slice(head[0].length));
    let unnamedPh = 0;
    for (const part of parts.slice(1)) {
      const name = /^:([^:]+):/.exec(part);
      const ph = name ? name[1] : unnamedPh++ === 0 ? 'PH' : `PH_${unnamedPh - 1}`;
      text += `{$${ph}}` + unquoteTemplate(name ? part.slice(name[0].length) : part);
    }
    messages.push({ id, source: text });
  }
  return { messages, unnamed };
}

function unquoteTemplate(text) {
  return text.replace(/\\(u\{[0-9a-fA-F]+\}|u[0-9a-fA-F]{4}|x[0-9a-fA-F]{2}|.)/g, (_, e) => {
    if (e[0] === 'u') return String.fromCodePoint(parseInt(e.replace(/[u{}]/g, ''), 16));
    if (e[0] === 'x') return String.fromCharCode(parseInt(e.slice(1), 16));
    return { n: '\n', t: '\t', r: '\r', 0: '\0' }[e] ?? e;
  });
}

// ---- the catalog: $localize messages and msg() messages together ----

/**
 * Joins message lists into one catalog: every id once. The same id with two different texts is an error (two messages
 * would share a translation). `prefix` keeps only ids in that namespace (`pd.` for the shell).
 */
export function buildCatalog(lists, prefix = '') {
  const byId = new Map();
  for (const list of lists) {
    for (const m of list) {
      if (prefix && !m.id.startsWith(prefix)) continue;
      const known = byId.get(m.id);
      if (known && known.source !== m.source) throw new Error(`message ${m.id} has two texts: ${JSON.stringify(known.source)} and ${JSON.stringify(m.source)}`);
      if (!known || (known.description === undefined && m.description !== undefined)) byId.set(m.id, m);
    }
  }
  return [...byId.values()].sort((a, b) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
}

/** Messages from a runtime JSON table (`{ locale, translations }`, what `localize-extract --format json` writes). */
export function messagesFromJson(json) {
  const t = json?.translations;
  if (typeof t !== 'object' || t === null) throw new Error('not a translation file: it has no "translations" object');
  return Object.entries(t).map(([id, source]) => {
    if (typeof source !== 'string') throw new Error(`message ${id} is not text`);
    return { id, source };
  });
}

/** Messages from a manifest (`paddock.manifest.json`, S2): its `messages` section, id -> English text. */
export function messagesFromManifest(manifest) {
  const m = manifest?.messages;
  if (typeof m !== 'object' || m === null) throw new Error('not a Paddock manifest: it has no "messages" section');
  return Object.entries(m).map(([id, source]) => ({ id, source: String(source) }));
}

/**
 * Merges runtime translation tables (the shell's and the app's) into one. The same id translated two ways is an error:
 * the app and the shell must not both own a message.
 */
export function mergeTranslations(tables) {
  const out = {};
  for (const t of tables) {
    for (const [id, text] of Object.entries(t)) {
      if (id in out && out[id] !== text) throw new Error(`message ${id} is translated twice, differently`);
      out[id] = text;
    }
  }
  return out;
}
