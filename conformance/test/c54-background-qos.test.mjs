import assert from 'node:assert/strict';
import { test } from 'node:test';
import { backgroundQos } from '../src/checks/C54-background-qos.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const BG = 'unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) };';

test('C54 bad: every QOS_CLASS_BACKGROUND in code, however it is reached', () => {
  assert.deepEqual(backgroundQos(`fn lower() {\n    ${BG}\n}\n`), [{ line: 2, waived: false }]);
  assert.deepEqual(backgroundQos('use libc::qos_class_t::QOS_CLASS_BACKGROUND;\nfn f() { set(QOS_CLASS_BACKGROUND) }\n').map((u) => u.line), [1, 2]);
});

test('C54 good: UTILITY, comments and strings', () => {
  assert.deepEqual(backgroundQos('fn lower() { set(QOS_CLASS_UTILITY) }\n'), []);
  assert.deepEqual(backgroundQos('// macOS was QOS_CLASS_BACKGROUND\n/// not `QOS_CLASS_BACKGROUND`\nlet m = "must not be QOS_CLASS_BACKGROUND";\n'), []);
});

test('C54 waivers: with a reason the use passes, without one it is reported', () => {
  assert.equal(backgroundQos(`// conformance: background-qos — cache sweeps nobody waits on\n${BG}`)[0].waived, 'reason');
  assert.equal(backgroundQos(`${BG} // conformance: background-qos`)[0].waived, 'no-reason');
});

test('C54 through the runner: an error per unwaived use, none for UTILITY or waived ones', async () => {
  const f = await findingsOf(
    'C54',
    makeApp({
      'crates/a/src/bad.rs': `fn lower() {\n    ${BG}\n}\n`,
      'crates/a/src/good.rs': 'fn lower() { set(QOS_CLASS_UTILITY) }\n',
      'crates/a/src/waived.rs': `// conformance: background-qos — idle maintenance nobody waits on\n${BG}\n`,
      'crates/a/src/bare.rs': `// conformance: background-qos\n${BG}\n`,
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]).sort(), [['crates/a/src/bad.rs', 2, 'error'], ['crates/a/src/bare.rs', 2, 'error']]);
  assert.match(f.find((x) => x.file.endsWith('bad.rs')).fix, /QOS_CLASS_UTILITY/);
  assert.match(f.find((x) => x.file.endsWith('bare.rs')).message, /no reason/);
});
