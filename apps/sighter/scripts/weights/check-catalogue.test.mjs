import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { check, parse, URL_PREFIX } from './check-catalogue.mjs';

const real = readFileSync(new URL('../../catalogue/model-weights.toml', import.meta.url), 'utf8');
const good = `format = 1
version = "9.9.9"
license = "Apache-2.0"

[[model]]
key = "a"
file = "a.pth"
url = "${URL_PREFIX}9.9.9/a.pth"
size = 5
sha256 = "${'ab'.repeat(32)}"
license = "MIT"
`;

test('the committed catalogue passes', () => assert.deepEqual(check(real), []));
test('a minimal good catalogue passes', () => assert.deepEqual(check(good), []));

const bad = (from, to) => check(good.replace(from, to));
test('each rule refuses the whole file', () => {
  assert.match(bad('size = 5', 'size = 0')[0], /size/);
  assert.match(bad('ab'.repeat(32), 'AB'.repeat(32))[0], /sha256/);
  assert.match(bad('ab'.repeat(32), 'ab'.repeat(31))[0], /sha256/);
  assert.match(bad('license = "MIT"', 'license = "GPL-3.0"')[0], /licence/);
  assert.match(bad('license = "Apache-2.0"', 'license = "PML-1.0"')[0], /top-level/);
  assert.match(bad('9.9.9/a.pth', '1.0.0/a.pth')[0], /url must be/);
  assert.match(bad('https://dev.decryptology.net', 'https://example.com')[0], /url must be/);
  assert.match(bad('format = 1', 'format = 2')[0], /format/);
  assert.match(bad('file = "a.pth"', 'file = "../a.pth"')[0], /file/);
});
test('garbage and duplicates are parse or key errors, never a pass', () => {
  assert.match(check(good + 'oops')[0], /does not parse/);
  assert.match(check(good + good.split('[[model]]')[0].replace('format = 1', '')).join('\n'), /duplicate key/);
  assert.match(check(good + '[[model]]\n' + good.split('[[model]]')[1])[0], /duplicate key/);
  assert.equal(check('format = 1\nversion = "1"\nlicense = "MIT"\n')[0], 'no [[model]] entries');
  assert.equal(parse('# c\n\nx = 3\n').top.x, 3);
});
