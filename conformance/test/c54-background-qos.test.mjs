import assert from 'node:assert/strict';
import { test } from 'node:test';
import { backgroundQos, STARVABLE } from '../src/checks/C54-background-qos.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const BG = 'unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) };';

test('C54 bad: every QOS_CLASS_BACKGROUND in code, however it is reached', () => {
  assert.deepEqual(backgroundQos(`fn lower() {\n    ${BG}\n}\n`), [{ line: 2, name: 'QOS_CLASS_BACKGROUND', waived: false }]);
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

test('C54 bad: the starvable levels of Windows and Linux too', () => {
  const win = [
    'use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};',
    'unsafe { SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN) };',
    'unsafe { SetPriorityClass(child, IDLE_PRIORITY_CLASS) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_IDLE) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };',
  ];
  const linux = ['unsafe { libc::sched_setscheduler(0, libc::SCHED_IDLE, &param) };', 'ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE << 13);'];
  const found = backgroundQos([...win, ...linux].join('\n')).map((u) => [u.line, u.name]);
  assert.deepEqual(found, [
    [1, 'THREAD_MODE_BACKGROUND_BEGIN'],
    [2, 'PROCESS_MODE_BACKGROUND_BEGIN'],
    [3, 'IDLE_PRIORITY_CLASS'],
    [4, 'THREAD_PRIORITY_IDLE'],
    [5, 'THREAD_PRIORITY_LOWEST'],
    [6, 'SCHED_IDLE'],
    [7, 'IOPRIO_CLASS_IDLE'],
  ]);
  assert.equal(Object.keys(STARVABLE).length, 9);
});

test('C54 good: the gentle levels of every OS and longer names that only contain a starvable one', () => {
  const ok = [
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };',
    'unsafe { SetPriorityClass(child, BELOW_NORMAL_PRIORITY_CLASS) };',
    'unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, 10) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };',
    'const MY_SCHED_IDLE_HINT: u32 = 0;',
  ];
  assert.deepEqual(backgroundQos(ok.join('\n')), []);
});

test('C54 through the runner: a Windows use is reported with its OS, a waived one passes', async () => {
  const f = await findingsOf(
    'C54',
    makeApp({
      'crates/a/src/win.rs': 'fn lower() {\n    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };\n}\n',
      'crates/a/src/idle.rs': '// conformance: background-qos — cache sweeps nobody waits on\nunsafe { libc::sched_setscheduler(0, libc::SCHED_IDLE, &p) };\n',
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.line]), [['crates/a/src/win.rs', 2]]);
  assert.match(f[0].message, /Windows thread background mode/);
  assert.match(f[0].fix, /THREAD_PRIORITY_BELOW_NORMAL/);
});

test('C54: macOS process background (PRIO_DARWIN_BG, taskpolicy -b) is flagged; ADR 0024 children carry the waiver', () => {
  const bad = [
    'unsafe { libc::setpriority(libc::PRIO_DARWIN_PROCESS, pid, libc::PRIO_DARWIN_BG) };',
    'let c = Command::new("taskpolicy").arg("-b").arg(exe).spawn()?;',
    'let c = Command::new("taskpolicy")',
    '    .args(["-b", "--", exe])',
    'sh(&format!("taskpolicy -b {exe}"));',
  ];
  assert.deepEqual(backgroundQos(bad.join('\n')).map((u) => [u.line, u.name]), [
    [1, 'PRIO_DARWIN_BG'],
    [2, 'taskpolicy -b'],
    [3, 'taskpolicy -b'],
    [5, 'taskpolicy -b'],
  ]);
  const ok = [
    '// taskpolicy -b would starve it; PRIO_DARWIN_BG too',
    'let c = Command::new("taskpolicy").arg("-c").arg("utility").arg(exe);',
    'let m = "not PRIO_DARWIN_BG";',
  ];
  assert.deepEqual(backgroundQos(ok.join('\n')), []);
  const child = '// conformance: background-qos — ADR 0024 Background child: an index rebuild nobody waits on\nunsafe { libc::setpriority(libc::PRIO_DARWIN_PROCESS, pid, libc::PRIO_DARWIN_BG) };';
  assert.equal(backgroundQos(child)[0].waived, 'reason');
});
