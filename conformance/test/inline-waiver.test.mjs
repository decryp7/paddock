import assert from 'node:assert/strict';
import test from 'node:test';
import { error } from '../src/finding.mjs';
import { applyInlineWaiver, commentView, commentsOf, inlineWaiver } from '../src/util/inline-waiver.mjs';
import { runChecks } from '../src/runner.mjs';
import { makeApp } from './helpers.mjs';

test('inlineWaiver: a reason on the same line is a waiver, a bare marker is not, another tag is nothing', () => {
  assert.deepEqual(inlineWaiver('x // conformance: t — because', 't'), { state: 'reason', reason: 'because' });
  assert.deepEqual(inlineWaiver('conformance: t — ab', 't'), { state: 'no-reason' });
  assert.deepEqual(inlineWaiver('conformance: t', 't'), { state: 'no-reason' });
  assert.deepEqual(inlineWaiver('conformance: other — because', 't'), { state: 'none' });
});

test('inlineWaiver: a dash at the end of a line does not take the next line as its reason, nor a list dash on the next line', () => {
  assert.deepEqual(inlineWaiver('// conformance: t —\nsome code here', 't'), { state: 'no-reason' });
  assert.deepEqual(inlineWaiver('# conformance: t\n- run: ./dev x', 't'), { state: 'no-reason' });
});

test('applyInlineWaiver: the site, the bare finding or the waived site', () => {
  const site = error('CX', 1, 'a', 3, 'site', 'fix');
  const bare = () => error('CX', 1, 'a', 3, 'bare', 'fix');
  assert.equal(applyInlineWaiver({ state: 'none' }, site, bare).message, 'site');
  assert.equal(applyInlineWaiver({ state: 'no-reason' }, site, bare).message, 'bare');
  assert.equal(applyInlineWaiver({ state: 'reason', reason: 'why' }, site, bare).inlineWaiver, 'why');
});

test('the runner lists an inline-waived site of any check under waived, with the reason, the file and the line', async () => {
  const check = {
    id: 'CX1',
    rule: 1,
    title: 'no TODO',
    scope: 'both',
    mode: 'gate',
    run(ctx) {
      const out = [];
      for (const f of ctx.files().filter((x) => x.endsWith('.txt'))) {
        const lines = ctx.read(f).split('\n');
        lines.forEach((l, i) => {
          if (!l.startsWith('TODO')) return;
          const near = `${l}\n${lines[i - 1] ?? ''}`;
          out.push(applyInlineWaiver(inlineWaiver(near, 'todo'), error('CX1', 1, f, i + 1, 'a TODO', 'Do it.'), () => error('CX1', 1, f, i + 1, 'the todo waiver gives no reason', 'Say why.')));
        });
      }
      return out;
    },
  };
  const root = makeApp({ 'a.txt': 'TODO a\n// conformance: todo — tracked in the backlog\nTODO b\nTODO c\n' });
  const r = await runChecks({ root, app: 'x', registry: [check] });
  assert.deepEqual(r.findings.map((f) => f.line), [1, 4]);
  assert.equal(r.waived.length, 1);
  assert.deepEqual([r.waived[0].file, r.waived[0].line, r.waived[0].waiver], ['a.txt', 3, { reason: 'tracked in the backlog', expires: 'inline' }]);
});

test('commentsOf: only comments are kept; a marker inside a string, raw string or char literal is data', () => {
  const rust = 'let a = "// conformance: t — x"; // conformance: t — real reason\nlet b = r#"/* conformance: t — y */"#;\n/* conformance: t — block */ let c = \'"\';\n';
  const c = commentsOf(rust, 'rust');
  assert.equal(c.length, rust.length);
  assert.deepEqual(c.split('\n').map((l) => l.trim()), ['// conformance: t — real reason', '', '/* conformance: t — block */', '']);
  const yaml = commentsOf('run: echo "# not a comment" # conformance: t — real\nx: 1#conformance: t — glued\nplain # one\nname: don\'t # two\n', 'yaml');
  assert.deepEqual(yaml.split('\n').map((l) => l.trim()), ['# conformance: t — real', '', '# one', '# two', '']);
  const shell = commentsOf("cat <<EOF\n# body\nEOF\necho $'a\\'b' # one\n", 'shell');
  assert.deepEqual(shell.split('\n').map((l) => l.trim()), ['', '', '', '# one', '']);
});

test('commentView: a waiver reads the comment on the line or the line above, never a string', () => {
  const v = commentView('let m = "conformance: t — in a string";\nlet x = 1;\n// conformance: t — above\nlet y = 2;\n', 'rust');
  assert.equal(v.waiver(1, 't').state, 'none');
  assert.equal(v.waiver(2, 't').state, 'none');
  assert.deepEqual(v.waiver(4, 't'), { state: 'reason', reason: 'above' });
  assert.equal(commentView('// conformance: f — whole file\nfn a() {}\n', 'rust').file('f').state, 'reason');
  assert.equal(commentView('let m = "conformance: f — whole file";\n', 'rust').file('f').state, 'none');
});
