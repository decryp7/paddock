import assert from 'node:assert/strict';
import { test } from 'node:test';
import { bindingFor, modKey, tabKey, walkStep } from './keyboard.ts';

test('a walk goes on while every stop is new', () => {
  assert.equal(walkStep([], 'a'), 'continue');
  assert.equal(walkStep(['a'], 'b'), 'continue');
  assert.equal(walkStep(['a', 'b', 'c'], 'd'), 'continue');
});

test('the focus leaving the page’s elements ends the walk cleanly', () => {
  assert.equal(walkStep([], null), 'left');
  assert.equal(walkStep(['a', 'b'], null), 'left');
});

test('coming back to the first stop is a cycle, as in a dialog or a menu', () => {
  assert.equal(walkStep(['a', 'b'], 'a'), 'cycled');
  assert.equal(walkStep(['a', 'b', 'c', 'd'], 'a'), 'cycled');
});

test('a stop that does not move, or a loop that skips the first stop, is a trap', () => {
  assert.equal(walkStep(['a'], 'a'), 'trapped');
  assert.equal(walkStep(['a', 'b'], 'b'), 'trapped');
  assert.equal(walkStep(['a', 'b', 'c'], 'b'), 'trapped');
  assert.equal(walkStep(['a', 'b', 'c', 'd'], 'c'), 'trapped');
});

test('WebKit on macOS reaches buttons and links only with Option held (full keyboard access is off by default), others with plain Tab', () => {
  assert.equal(tabKey('webkit', 'darwin'), 'Alt+Tab');
  assert.equal(tabKey('webkit', 'darwin', true), 'Alt+Shift+Tab');
  assert.equal(tabKey('chromium', 'darwin'), 'Tab');
  assert.equal(tabKey('webkit', 'linux'), 'Tab');
  assert.equal(tabKey('webkit', 'win32', true), 'Shift+Tab');
  assert.equal(tabKey('firefox', 'darwin'), 'Tab');
});

// Regression (CI run 7134, Dyno's WebKit leg on Linux): `devices['Desktop Safari']` reports a Mac, so the app binds `Mod` to
// Cmd, while Playwright's `ControlOrMeta` follows the machine the tests run on and pressed Ctrl. The key follows the page.
test('Mod is the key the page binds it to: Cmd for a Mac or iOS navigator, Ctrl for any other', () => {
  const safari = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/26.0 Safari/605.1.15';
  const chrome = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36';
  assert.equal(modKey({ platform: 'Linux x86_64', userAgent: safari }), 'Meta');
  assert.equal(modKey({ platform: 'MacIntel', userAgent: '' }), 'Meta');
  assert.equal(modKey({ platform: 'iPad', userAgent: '' }), 'Meta');
  assert.equal(modKey({ platform: 'MacIntel', userAgent: chrome }), 'Meta');
  assert.equal(modKey({ platform: 'Win32', userAgent: chrome }), 'Control');
  assert.equal(modKey({ platform: 'Linux x86_64', userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/605.1.15' }), 'Control');
  assert.equal(modKey({}), 'Control');
});

test('a binding keeps every key and swaps only its Mod, wherever the combination puts it', () => {
  assert.equal(bindingFor('Mod+Shift+P', 'Meta'), 'Meta+Shift+P');
  assert.equal(bindingFor('Mod+Alt+K', 'Control'), 'Control+Alt+K');
  assert.equal(bindingFor('Mod+Comma', 'Meta'), 'Meta+Comma');
  assert.equal(bindingFor('Shift+Mod+z', 'Control'), 'Shift+Control+z');
  assert.equal(bindingFor('Mod+Enter', 'Meta'), 'Meta+Enter');
  assert.equal(bindingFor('Mod++', 'Meta'), 'Meta++');
  assert.equal(bindingFor('Mod', 'Meta'), 'Mod', 'a lone Mod is a key name, not a modifier');
  assert.equal(bindingFor('Shift+z', 'Meta'), 'Shift+z');
});
