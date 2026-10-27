// Fails the bundle when the built index.html has inline <style> or <script> elements. Tauri adds a nonce or hash
// for each to the CSP, and a style-src with a nonce makes browsers ignore 'unsafe-inline', which blocks the
// <style> elements Angular adds at run time: the app then renders without its component styles.
// Shared by every Tauri + Angular app (rule 42; it was a per-app copy). Run it from the app's folder as Tauri's
// beforeBuildCommand after `ng build`: node ../../../conformance/ci/check-index.mjs [dist/app/browser/index.html]
import { readFileSync } from 'node:fs';
import { isMain } from '../src/util/is-main.mjs';

export function inlineElements(html) {
  const found = [];
  for (const m of html.matchAll(/<(style|script)\b([^>]*)>/gi)) {
    if (m[1].toLowerCase() === 'style' || !/\bsrc\s*=/.test(m[2])) found.push(`<${m[1]}${m[2]}>`);
  }
  return found;
}

if (isMain(import.meta.url)) {
  const file = process.argv[2] ?? 'dist/app/browser/index.html';
  const found = inlineElements(readFileSync(file, 'utf8'));
  if (found.length) {
    console.error(`${file} has inline elements that break the app's styles under Tauri's CSP:\n  ${found.join('\n  ')}`);
    console.error('Keep "inlineCritical": false in angular.json (production optimization.styles).');
    process.exit(1);
  }
}
