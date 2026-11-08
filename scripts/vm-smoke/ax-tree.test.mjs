import assert from 'node:assert/strict';
import { test } from 'node:test';
import { parseTree, rawTextOf } from './ax-tree.mjs';

test('parses roles and labels, drops the _NS id', () => {
  const rows = parseTree('AXWindow  Dyno\n  AXWebArea  Dashboard – Dyno\n    AXButton  Turn on administrator readings | _NS:41\n    AXStaticText  On | _NS:9\n  AXGroup\n');
  assert.deepEqual(rows.map((r) => [r.role, r.text]), [['AXWindow', 'Dyno'], ['AXWebArea', 'Dashboard – Dyno'], ['AXButton', 'Turn on administrator readings'], ['AXStaticText', 'On']]);
  assert.deepEqual(rawTextOf(rows).map((r) => r.text), ['Turn on administrator readings', 'On']);
});
