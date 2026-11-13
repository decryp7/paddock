import assert from 'node:assert/strict';
import { test } from 'node:test';
import { flagged, kindOf, parseAllow } from '../src/checks/C121-declared-data.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const ALLOW = JSON.stringify({ allow: [{ path: 'conformance/test/', reason: 'tests of checks of workflows' }] });

test('C121 kinds: registries and test files are read, fixtures and other files are not', () => {
  assert.equal(kindOf('docs/generators.toml'), 'registry');
  assert.equal(kindOf('docs/sub/x.toml'), null);
  for (const t of ['tools/x/tests/it/a.rs', 'conformance/test/a.test.mjs', 'scripts/affected.test.mjs', 'src/queue/tests.rs', 'a/b_test.rs', '.claude/hooks/x.test.sh']) {
    assert.equal(kindOf(t), 'test', t);
  }
  for (const n of ['src/lib.rs', 'tools/x/tests/fixtures/a.rs', 'tests/a.toml', 'node_modules/x/tests/a.js']) assert.equal(kindOf(n), null, n);
});

test('C121 bad: a registry or a test that runs a text tool over a workflow (bddd2947c)', () => {
  const sed = `check = ["sh", "-c", "args=$(sed -n 's/^X: \\"\\(.*\\)\\"$/\\1/p' .gitea/workflows/ci.yml | head -1)"]`;
  assert.deepEqual(flagged('registry', `a = 1\n${sed}\n`), [{ line: 2, what: '`sed`' }]);
  assert.deepEqual(flagged('test', 'Command::new("grep").arg("x").arg(".gitea/workflows/ci.yml");\n'), [{ line: 1, what: '`grep`' }]);
  assert.deepEqual(flagged('registry', 'check = ["sh", "-c", "rg -n x workflows/ci.yml"]\n'), [{ line: 1, what: '`rg`' }]);
});

test('C121 bad: a regular expression over a workflow, in a test only', () => {
  const line = 'const hit = readFileSync(".gitea/workflows/ci.yml", "utf8").match(/x/);\n';
  assert.deepEqual(flagged('test', line), [{ line: 1, what: 'a regular expression' }]);
  assert.deepEqual(flagged('registry', line), []);
  assert.deepEqual(flagged('test', 'let re = Regex::new("x"); let t = include_str!(".gitea/workflows/ci.yml");\n'), [{ line: 1, what: 'a regular expression' }]);
});

test('C121 good: comments, other files and the accessor', () => {
  assert.deepEqual(flagged('registry', '# sed over .gitea/workflows/ci.yml in a comment\n'), []);
  assert.deepEqual(flagged('test', '// grep .gitea/workflows/ci.yml\n'), []);
  assert.deepEqual(flagged('registry', 'check = ["sh", "-c", "args=$(./dev ci conformance-args) || exit $?; exec node $args"]\n'), []);
  assert.deepEqual(flagged('test', 'sed -n p src/lib.rs\n'), []);
  assert.deepEqual(flagged('test', 'const t = readFileSync(".gitea/workflows/ci.yml", "utf8");\n'), []);
  // A word that only contains a tool name is not the tool.
  assert.deepEqual(flagged('registry', 'what = "the sedate .gitea/workflows/ci.yml reader"\n'), []);
});

test('C121 allow list: needs a path and a reason, matches a folder or a file', () => {
  assert.deepEqual(parseAllow(ALLOW), [{ path: 'conformance/test/', reason: 'tests of checks of workflows' }]);
  assert.throws(() => parseAllow('{"allow":[{"path":"x","reason":"short"}]}'), /reason of at least 8/);
  assert.throws(() => parseAllow('{}'), /no `allow` array/);
  assert.throws(() => parseAllow('nope'), /not JSON/);
});

test('C121 through the runner: warnings only, the allow list spares its folder, a missing list is a broken check', async () => {
  const root = makeApp({
    'conformance/c121-allow.json': ALLOW,
    'docs/g.toml': 'check = ["sh", "-c", "sed -n p .gitea/workflows/ci.yml"]\n',
    'conformance/test/x.test.mjs': 'const t = text.match(/x/); // .gitea/workflows/ci.yml\n',
    'tools/t/tests/it/a.rs': 'let _ = Regex::new("x"); // .gitea/workflows/ci.yml\n',
  });
  const found = await findingsOf('C121', root);
  assert.deepEqual(found.map((f) => [f.file, f.line, f.severity]), [['docs/g.toml', 1, 'warn'], ['tools/t/tests/it/a.rs', 1, 'warn']]);
  const none = makeApp({ 'docs/g.toml': 'a = 1\n' });
  const broken = await findingsOf('C121', none);
  assert.equal(broken.length, 1);
  assert.equal(broken[0].unwaivable, true);
  assert.match(broken[0].message, /cannot look/);
});
