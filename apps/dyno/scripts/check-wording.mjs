#!/usr/bin/env node
// Dyno shows readings and runs only the power actions its owner sets up: nothing it shows may promise tuning, cleanup or another change to the computer (ER-21: the class is checked, not each string).
// Scans the user-visible text of the Rust core and helper (string literals), the Angular app (templates and TS strings),
// the Tauri bundle descriptions and the Cargo descriptions for change vocabulary, minus an explicit allow-list.
//
//   node scripts/check-wording.mjs [apps/dyno]
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Words a monitor that does not tune or clean up must not use in text people read. */
export const FORBIDDEN = [
  /\btunes?\b/i,
  /\btuning\b/i,
  /\bundo(?:ne|able)?\b/i,
  /\bapply\b/i,
  /\bapplied\b/i,
  /\bapplies\b/i,
  /\bquarantine[sd]?\b/i,
  /enable a rule/i,
  /end process/i,
  /change your pc/i,
  /fix it/i,
];

/**
 * Legitimate hits, each with the reason. A rule is `{ file, text }`: the file (relative to apps/dyno, a suffix match) and a
 * substring of the offending line.
 */
export const ALLOW = [
  // Plugin states describe a plugin the host stopped after repeated crashes; nothing on the computer is quarantined.
  { file: 'app/src/app/core/api.types.ts', text: 'quarantined', reason: 'the plugin host stops a crashing plugin: a state name, not a change to the computer' },
  { file: 'app/src/app/core/mock/data.ts', text: 'quarantined', reason: 'mock of that plugin state' },
  { file: 'app/src/app/state/counters.store.ts', text: 'Undo', reason: 'undoes deleting one of Dyno\'s own saved counter sets, a setting of Dyno, not of the computer' },
  // The popup, dashboard and menu-bar widget layouts are Dyno's own settings (ADR 0012, family ADR 0035): an edit can be undone, and nothing on the computer changes.
  { file: 'app/src/app/core/generated/widgets.ts', text: 'resetToDefaults undo', reason: 'wire name of the layout edit "undo" (generated from paddock-widgets)' },
  { file: 'app/src/app/core/mock/widgets.ts', text: "undo", reason: 'mock of that layout edit and its refusal' },
  { file: 'app/src/app/features/settings/settings-page.ts', text: 'Undo is in the message that follows', reason: 'a widget layout edit is saved at once and can be undone: a setting of Dyno, not of the computer' },
  { file: 'app/src/app/features/settings/menu-bar-items.ts', text: 'Undo is in the message that follows', reason: 'the same widget layout edit, on the menu-bar items page: saved at once, undone from Dyno\'s own message; a setting of Dyno, not of the computer' },
  { file: 'app/src/app/core/api.types.ts', text: 'tune retire', reason: 'wire value of the alarm statistics hint kind: adjust an alarm\'s own threshold' },
  { file: 'app/src/app/core/mock/alarms.ts', text: 'tune GPU load', reason: 'mock of that wire value' },
  // Landed with the widget grid, the benchmarks, the lists and the disk-health tool.
  { file: 'app/src/app/core/generated/widgets.ts', text: 'undo redo', reason: 'wire names of the layout edits Undo and Redo (generated from paddock-widgets): Dyno\'s own layout' },
  { file: 'app/src/app/features/benchmarks/bench-page.ts', text: 'undo', reason: 'the name of an icon (a cancelled run is shown with the undo icon): no change to the computer' },
  { file: 'app/src/app/features/lists/lists-page.ts', text: 'applyDedupe', reason: 'removes duplicate entries from the person\'s own list, which is Dyno\'s data, not the computer' },
  { file: 'crates/dyno-agent/src/tool_own_copy.rs', text: 'quarantine', reason: 'the macOS download flag of Dyno\'s own extracted copy of the tool (its own folder, removed with it)' },
  { file: 'crates/dyno-plugin-host/Cargo.toml', text: 'quarantine', reason: 'the plugin host stops a crashing plugin (a state of the plugin, not a change to the computer)' },
];

const SKIP_DIRS = new Set(['node_modules', 'target', 'dist', '.angular', 'docs', 'diagrams', 'e2e', '.serena']);

function* walk(dir) {
  for (const name of readdirSync(dir)) {
    if (SKIP_DIRS.has(name)) continue;
    const p = join(dir, name);
    const st = statSync(p);
    if (st.isDirectory()) yield* walk(p);
    else yield p;
  }
}

const isTest = (p) => /\.spec\.ts$|\/tests\/|\/tests\.rs$/.test(p);

/** Lines of `text` that are user-visible, with their 1-based numbers. */
export function visibleLines(path, text) {
  const out = [];
  const lines = text.split('\n');
  if (path.endsWith('.rs')) {
    // Only what sits inside a string literal; test modules are skipped (they pin the words that must not appear).
    const cut = text.indexOf('#[cfg(test)]');
    const upto = cut === -1 ? lines.length : text.slice(0, cut).split('\n').length - 1;
    for (let i = 0; i < upto; i++) {
      const strings = [...lines[i].matchAll(/"((?:[^"\\]|\\.)*)"/g)].map((m) => m[1]);
      if (strings.length && !lines[i].trim().startsWith('//')) out.push([i + 1, strings.join(' ')]);
      // A doc comment on a wire type is shown in generated docs only, not to people.
    }
  } else if (path.endsWith('.ts')) {
    // Only what sits inside a quote or a template (the words a person can read); identifiers and comments are not shown.
    // Icon names (`name="undo"`) are asset keys, not words.
    let inTemplate = false;
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i];
      if (!inTemplate && /^\s*(\/\/|\*|\/\*\*)/.test(l)) continue;
      const pieces = [];
      let k = 0;
      while (k < l.length) {
        if (inTemplate) {
          const end = l.indexOf('`', k);
          pieces.push(l.slice(k, end === -1 ? l.length : end));
          if (end === -1) break;
          inTemplate = false;
          k = end + 1;
        } else {
          const m = /(['"`])/.exec(l.slice(k));
          if (!m) break;
          const at = k + m.index;
          if (m[1] === '`') {
            inTemplate = true;
            k = at + 1;
          } else {
            let e = at + 1;
            while (e < l.length && l[e] !== m[1]) e += l[e] === '\\' ? 2 : 1;
            pieces.push(l.slice(at + 1, e));
            k = e + 1;
          }
        }
      }
      const text = pieces.join(' ').replace(/\bname="[\w-]+"/g, '');
      if (text.trim()) out.push([i + 1, text]);
    }
  } else if (path.endsWith('tauri.conf.json')) {
    lines.forEach((l, i) => {
      if (/"(shortDescription|longDescription)"/.test(l)) out.push([i + 1, l]);
    });
  } else if (path.endsWith('Cargo.toml')) {
    lines.forEach((l, i) => {
      if (/^description\s*=/.test(l)) out.push([i + 1, l]);
    });
  }
  return out;
}

/** Offences under `root` (apps/dyno): `{ file, line, word, text }`. */
export function findOffences(root, allow = ALLOW) {
  const found = [];
  for (const abs of walk(root)) {
    const rel = relative(root, abs);
    const wanted =
      (rel.startsWith('crates/dyno-core/src/') || rel.startsWith('crates/dyno-helper/src/') || rel.startsWith('crates/dyno-agent/src/')) && rel.endsWith('.rs') ||
      (rel.startsWith('app/src/app/') && rel.endsWith('.ts')) ||
      rel === 'app/src-tauri/tauri.conf.json' ||
      /(^|\/)Cargo\.toml$/.test(rel) && !rel.startsWith('plugins/');
    if (!wanted || isTest(rel)) continue;
    for (const [line, text] of visibleLines(rel, readFileSync(abs, 'utf8'))) {
      for (const re of FORBIDDEN) {
        const m = re.exec(text);
        if (!m) continue;
        if (allow.some((a) => rel.endsWith(a.file) && text.includes(a.text))) continue;
        found.push({ file: rel, line, word: m[0], text: text.trim().slice(0, 160) });
      }
    }
  }
  return found;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const root = process.argv[2] ?? new URL('..', import.meta.url).pathname;
  const offences = findOffences(root);
  for (const o of offences) console.error(`${o.file}:${o.line}: "${o.word}" in: ${o.text}`);
  if (offences.length) {
    console.error(`${offences.length} place(s) promise a change Dyno does not make. Reword them, or allow-list one with a reason in scripts/check-wording.mjs.`);
    process.exit(1);
  }
  console.log('Dyno only talks about looking.');
}
