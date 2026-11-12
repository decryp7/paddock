// The workflow reader (lib/workflow.mjs): what it reads, and that an intended edit leaves a field assertion green while a real
// change turns it red.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { actionScript, conjuncts, expr, needsOf, parseOutputs, parseYaml, stepsOf, withs } from './lib/workflow.mjs';

const WF = `
on:
  push:
    tags: ["app-v*"]   # a tag only
  workflow_dispatch:
jobs:
  a:
    needs: [x, y]
    if: \${{ always() && needs.x.result == 'success' }}
    steps:
      - uses: ./act
        with:
          k: \${{ needs.x.outputs.v }}
      - name: go
        run: |
          echo "a: b" # kept
          echo done
  b:
    needs:
      - a
    outputs:
      o: \${{ steps.s.outputs.o }}
    strategy:
      matrix:
        include: \${{ fromJSON(needs.a.outputs.m) }}
`;

test('reads maps, lists, flow forms, comments, block scalars and expressions', () => {
  const w = parseYaml(WF);
  assert.deepEqual(w.on.push.tags, ['app-v*']);
  assert.equal(w.on.workflow_dispatch, null);
  assert.deepEqual(needsOf(w.jobs.a), ['x', 'y']);
  assert.deepEqual(needsOf(w.jobs.b), ['a']);
  assert.deepEqual(conjuncts(w.jobs.a.if), ['always()', "needs.x.result == 'success'"]);
  assert.equal(stepsOf(w.jobs.a)[1].run, 'echo "a: b" # kept\necho done\n');
  assert.equal(expr(w.jobs.b.strategy.matrix.include), 'fromJSON(needs.a.outputs.m)');
  assert.deepEqual(withs(w).map((x) => x.where), ['a/./act']);
  assert.equal(actionScript(w.jobs.a, 'go'), 'echo "a: b" # kept\necho done\n');
});

test('anchors and aliases are refused, never read wrongly', () => {
  assert.throws(() => parseYaml('a: &x 1\nb: *x\n'), /anchors/);
});

test('conjuncts keep parenthesised and quoted && whole; expr ignores spacing and the wrapper', () => {
  assert.deepEqual(conjuncts("a && (b && c) && d == 'x && y'"), ['a', '(b && c)', "d == 'x && y'"]);
  assert.equal(expr('${{   a  &&   b }}'), expr('a && b'));
});

test('GITHUB_OUTPUT is read as a map, heredoc values and later writes included', () => {
  assert.deepEqual(parseOutputs('run=true\nlist<<EOF\na\nb\nEOF\nrun=false\n'), { run: 'false', list: 'a\nb' });
});

test('an intended edit keeps a field assertion green; a real change turns it red', () => {
  const check = (text) => {
    const j = parseYaml(text).jobs.a;
    assert.deepEqual([...needsOf(j)].sort(), ['x', 'y']);
    assert.ok(conjuncts(j.if).includes("needs.x.result == 'success'"));
  };
  check(WF);
  check(WF.replace('needs: [x, y]', 'needs:\n      - y # reordered\n      - x').replace("&& needs.x.result == 'success'", "&& needs.x.result == 'success' && github.actor != 'bot'"));
  assert.throws(() => check(WF.replace('needs: [x, y]', 'needs: [x]')));
  assert.throws(() => check(WF.replace("needs.x.result == 'success'", 'true')));
});
