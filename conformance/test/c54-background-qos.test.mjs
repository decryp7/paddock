import assert from 'node:assert/strict';
import { test } from 'node:test';
import { STARVABLE, starvableUses } from '../src/checks/C54-background-qos.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const BG = 'unsafe { libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_BACKGROUND, 0) };';
const rs = (text) => starvableUses(text, 'src/x.rs');
const names = (text, file = 'src/x.rs') => starvableUses(text, file).map((u) => [u.line, u.name]);
const plist = (body) => `<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0">\n<dict>\n${body}\n</dict>\n</plist>\n`;

// ---- identifiers -----------------------------------------------------------------------------------------------------

test('C54 bad: every QOS_CLASS_BACKGROUND in code, however it is reached', () => {
  assert.deepEqual(rs(`fn lower() {\n    ${BG}\n}\n`), [{ line: 2, name: 'QOS_CLASS_BACKGROUND', waived: false }]);
  assert.deepEqual(rs('use libc::qos_class_t::QOS_CLASS_BACKGROUND;\nfn f() { set(QOS_CLASS_BACKGROUND) }\n').map((u) => u.line), [1, 2]);
});

test('C54 good: UTILITY, comments and strings', () => {
  assert.deepEqual(rs('fn lower() { set(QOS_CLASS_UTILITY) }\n'), []);
  assert.deepEqual(rs('// macOS was QOS_CLASS_BACKGROUND\n/// not `QOS_CLASS_BACKGROUND`\nlet m = "must not be QOS_CLASS_BACKGROUND";\n'), []);
});

test('C54 bad: the starvable levels of Windows, Linux and macOS I/O', () => {
  const lines = [
    'use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};',
    'unsafe { SetPriorityClass(GetCurrentProcess(), PROCESS_MODE_BACKGROUND_BEGIN) };',
    'unsafe { SetPriorityClass(child, IDLE_PRIORITY_CLASS) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_IDLE) };',
    'unsafe { libc::sched_setscheduler(0, libc::SCHED_IDLE, &param) };',
    'ioprio_set(IOPRIO_WHO_PROCESS, 0, IOPRIO_CLASS_IDLE << 13);',
    'unsafe { libc::setpriority(libc::PRIO_DARWIN_PROCESS, pid, libc::PRIO_DARWIN_BG) };',
    'unsafe { setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, IOPOL_THROTTLE) };',
    'SetProcessInformation(h, ProcessPowerThrottling, &state as *const _ as _, size);',
    'SetThreadInformation(h, ThreadPowerThrottling, &state as *const _ as _, size);',
  ];
  assert.deepEqual(names(lines.join('\n')), [
    [1, 'THREAD_MODE_BACKGROUND_BEGIN'],
    [2, 'PROCESS_MODE_BACKGROUND_BEGIN'],
    [3, 'IDLE_PRIORITY_CLASS'],
    [4, 'THREAD_PRIORITY_IDLE'],
    [5, 'SCHED_IDLE'],
    [6, 'IOPRIO_CLASS_IDLE'],
    [7, 'PRIO_DARWIN_BG'],
    [8, 'IOPOL_THROTTLE'],
    [9, 'ProcessPowerThrottling'],
    [10, 'ThreadPowerThrottling'],
  ]);
  assert.ok(!('THREAD_PRIORITY_LOWEST' in STARVABLE), 'LOWEST is slowed, not starved (rule 52)');
});

test('C54 good: the gentle levels of every OS, LOWEST, and longer names that only contain a starvable one', () => {
  const ok = [
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };',
    'unsafe { SetPriorityClass(child, BELOW_NORMAL_PRIORITY_CLASS) };',
    'unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, 10) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_END) };',
    'const MY_SCHED_IDLE_HINT: u32 = 0;',
    'unsafe { setiopolicy_np(IOPOL_TYPE_DISK, IOPOL_SCOPE_THREAD, IOPOL_UTILITY) };',
  ];
  assert.deepEqual(rs(ok.join('\n')), []);
});

test('C54 bad: raw numbers for the starvable Windows levels', () => {
  const lines = [
    'cmd.creation_flags(0x40);',
    'cmd.creation_flags(0x0010_0000 | CREATE_NO_WINDOW.0);',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY(0x0001_0000)) };',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY(-15)) };',
    'unsafe { SetPriorityClass(child, PROCESS_CREATION_FLAGS(64)) };',
  ];
  assert.deepEqual(names(lines.join('\n')), [
    [1, 'IDLE_PRIORITY_CLASS (0x40)'],
    [2, 'PROCESS_MODE_BACKGROUND_BEGIN (0x100000)'],
    [3, 'THREAD_MODE_BACKGROUND_BEGIN (0x10000)'],
    [4, 'THREAD_PRIORITY_IDLE (-15)'],
    [5, 'IDLE_PRIORITY_CLASS (0x40)'],
  ]);
  // 0x10000 as a creation flag is INHERIT_PARENT_AFFINITY, and 0x4000 is BELOW_NORMAL_PRIORITY_CLASS.
  assert.deepEqual(rs('cmd.creation_flags(0x0001_0000);\ncmd.creation_flags(0x4000 | 0x0800_0000);\nTHREAD_PRIORITY(-1)\n'), []);
});

// ---- commands --------------------------------------------------------------------------------------------------------

test('C54 bad: taskpolicy, ionice and chrt at their starvable levels, by basename', () => {
  const lines = [
    'let c = Command::new("taskpolicy").arg("-b").arg(exe).spawn()?;',
    'let c = Command::new("/usr/sbin/taskpolicy")',
    '    .args(["-c", "background", "--", exe])',
    '    .spawn()?;',
    'sh(&format!("taskpolicy -b {exe}"));',
    'Command::new("ionice").args(["-c3", "-p", &pid]);',
    'Command::new("ionice").args(["-c", "3", "-p", &pid]);',
    'Command::new("chrt").args(["--idle", "0", exe]);',
    'sh("nice -n 19 chrt -i 0 make");',
  ];
  assert.deepEqual(names(lines.join('\n')), [
    [1, 'taskpolicy -b'],
    [2, 'taskpolicy -c background'],
    [5, 'taskpolicy -b'],
    [6, 'ionice -c3'],
    [7, 'ionice -c3'],
    [8, 'chrt --idle'],
    [9, 'chrt --idle'],
  ]);
});

test('C54 good: gentle command levels, comments, and the name inside other text', () => {
  const ok = [
    '// taskpolicy -b would starve it; PRIO_DARWIN_BG too',
    'let c = Command::new("taskpolicy").arg("-c").arg("utility").arg(exe);',
    'let m = "not PRIO_DARWIN_BG";',
    'Command::new("ionice").args(["-c2", "-n7", "-p", &pid]);',
    'let help = "see the taskpolicy man page";',
    'Command::new("chrt").args(["--batch", "0", exe]);',
  ];
  assert.deepEqual(rs(ok.join('\n')), []);
});

// ---- services --------------------------------------------------------------------------------------------------------

test('C54 service settings: launchd Background, Adaptive without XPC, low-priority I/O, systemd idle CPU and I/O', () => {
  assert.deepEqual(names(plist('    <key>ProcessType</key>\n    <string>Background</string>'), 'a.plist'), [[4, 'launchd ProcessType Background']]);
  assert.deepEqual(names(plist('    <key>ProcessType</key>\n    <string>Adaptive</string>'), 'a.plist'), [[4, 'launchd ProcessType Adaptive without XPC']]);
  assert.deepEqual(names(plist('    <key>MachServices</key>\n    <dict><key>net.x</key><true/></dict>\n    <key>ProcessType</key>\n    <string>Adaptive</string>'), 'a.plist'), []);
  assert.deepEqual(names(plist('    <key>LowPriorityIO</key>\n    <true/>'), 'a.plist'), [[4, 'launchd LowPriorityIO']]);
  assert.deepEqual(names(plist('    <key>LowPriorityBackgroundIO</key>\n    <true/>'), 'a.plist'), [[4, 'launchd LowPriorityBackgroundIO']]);
  assert.deepEqual(names(plist('    <key>LowPriorityIO</key>\n    <false/>'), 'a.plist'), []);
  assert.deepEqual(names('[Service]\nNice=10\nIOSchedulingClass=idle\n', 'a.service'), [[3, 'systemd IOSchedulingClass=idle']]);
  assert.deepEqual(names('[Service]\nIOSchedulingClass = 3\nCPUSchedulingPolicy=idle\n', 'a.service'), [
    [2, 'systemd IOSchedulingClass=idle'],
    [3, 'systemd CPUSchedulingPolicy=idle'],
  ]);
  assert.deepEqual(names('[Service]\n# IOSchedulingClass=idle\nIOSchedulingClass=best-effort\nCPUSchedulingPolicy=batch\n', 'a.service'), []);
  assert.deepEqual(names(plist('    <!-- <key>LowPriorityIO</key><true/> -->\n    <key>ProcessType</key>\n    <string>Standard</string>'), 'a.plist'), []);
});

test('C54 service settings: MachServices counts per plist, not per file', () => {
  const two = [
    'const A: &str = "<plist><dict><key>MachServices</key><dict/><key>ProcessType</key><string>Adaptive</string></dict></plist>";',
    'const B: &str = "<plist><dict><key>ProcessType</key><string>Adaptive</string></dict></plist>";',
  ].join('\n');
  assert.deepEqual(names(two), [[2, 'launchd ProcessType Adaptive without XPC']]);
});

test('C54 service settings in Rust: the text a renderer writes, with its value, not its comments', () => {
  assert.deepEqual(names('fn unit(o: &mut String) {\n    o.push_str("Nice=10\\nCPUWeight=20\\nIOSchedulingClass=idle\\n");\n}\n'), [[2, 'systemd IOSchedulingClass=idle']]);
  assert.deepEqual(names('const P: &str = "\\t<key>ProcessType</key>\\n\\t<string>Background</string>\\n";\n'), [[1, 'launchd ProcessType Background']]);
  assert.deepEqual(names('fn f(o: &mut String) {\n    key(o, "LowPriorityIO");\n    o.push_str("    <true/>\\n");\n}\n'), [[2, 'launchd LowPriorityIO']]);
  assert.deepEqual(names('fn f(o: &mut String) {\n    key(o, "LowPriorityIO");\n    o.push_str("    <false/>\\n");\n}\n'), []);
  assert.deepEqual(names('fn parse(t: &str) -> bool { t.contains("LowPriorityIO") }\n'), []);
  assert.deepEqual(names('// writes IOSchedulingClass=idle and <key>LowPriorityIO</key>\nfn f() {}\n'), []);
  assert.deepEqual(names('let spec = ServiceSpec::builder("x")\n    .process_type(ProcessType::Background)\n    .build();\n'), [[2, 'paddock-service ProcessType::Background']]);
  assert.deepEqual(names('    .process_type(ProcessType::Interactive)\n'), []);
});

test('C54 service settings: tests and test files are skipped, the code under test is not', () => {
  const code = 'fn render(o: &mut String) { o.push_str("IOSchedulingClass=idle\\n"); }\n';
  const tests = '#[cfg(test)]\nmod tests {\n    #[test]\n    fn it() { assert!(render().contains("IOSchedulingClass=idle\\n")); }\n}\n';
  assert.deepEqual(names(code + tests), [[1, 'systemd IOSchedulingClass=idle']]);
  assert.deepEqual(names('[Service]\nIOSchedulingClass=idle\n', 'crates/a/tests/golden/a.service'), []);
  assert.deepEqual(names(code, 'crates/a/tests/it/render.rs'), []);
});

// ---- waivers ---------------------------------------------------------------------------------------------------------

test('C54 waivers: with a reason the use passes, without one it is reported', () => {
  assert.equal(rs(`// conformance: background-qos — cache sweeps nobody waits on\n${BG}`)[0].waived, 'reason');
  assert.equal(rs(`${BG} // conformance: background-qos`)[0].waived, 'no-reason');
});

test('C54 waivers: a trailing waiver covers its own line only, and a waiver in a string is no waiver', () => {
  const trailing = `set(QOS_CLASS_UTILITY); // conformance: background-qos — idle sweep\n${BG}`;
  assert.equal(rs(trailing)[0].waived, false);
  const inString = `let s = "// conformance: background-qos — nobody waits";\n${BG}`;
  assert.equal(rs(inString)[0].waived, false);
  const sameLineString = `${BG} let s = "conformance: background-qos — nobody waits";`;
  assert.equal(rs(sameLineString)[0].waived, false);
});

test('C54 waivers: an inline waiver that waives nothing is reported', async () => {
  const f = await findingsOf('C54', makeApp({ 'src/stale.rs': '// conformance: background-qos — the old idle sweep\nset(QOS_CLASS_UTILITY);\n' }));
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]), [['src/stale.rs', 1, 'warn']]);
  assert.match(f[0].message, /waives nothing/);
});

// ---- the runner ------------------------------------------------------------------------------------------------------

test('C54 through the runner: an error per unwaived use, none for UTILITY or waived ones', async () => {
  const f = await findingsOf(
    'C54',
    makeApp({
      'crates/a/src/bad.rs': `fn lower() {\n    ${BG}\n}\n`,
      'crates/a/src/good.rs': 'fn lower() { set(QOS_CLASS_UTILITY) }\n',
      'crates/a/src/waived.rs': `// conformance: background-qos — idle maintenance nobody waits on\n${BG}\n`,
      'crates/a/src/bare.rs': `// conformance: background-qos\n${BG}\n`,
      'macos/agent.plist': plist('    <key>ProcessType</key>\n    <string>Background</string>'),
      'linux/agent.service': '[Service]\n# conformance: background-qos — nightly cache sweep nobody waits on\nIOSchedulingClass=idle\n',
      'win/w.rs': 'fn lower() {\n    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };\n}\n',
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.line, x.severity]).sort(), [
    ['crates/a/src/bad.rs', 2, 'error'],
    ['crates/a/src/bare.rs', 2, 'error'],
    ['macos/agent.plist', 4, 'error'],
    ['win/w.rs', 2, 'error'],
  ]);
  assert.match(f.find((x) => x.file.endsWith('bad.rs')).fix, /QOS_CLASS_UTILITY/);
  assert.match(f.find((x) => x.file.endsWith('bare.rs')).message, /no reason/);
  assert.match(f.find((x) => x.file.endsWith('w.rs')).message, /Windows thread background mode/);
});

// ---- review round 3: prefixes, constants, pairing, arms, argument lists -----------------------------------------------

test('C54 systemd Exec lines: each prefix belongs to the first word only, and `@` skips argv[0]', () => {
  for (const p of ['', '-', '@', ':', '+', '!', '!!', '-@']) {
    const argv0 = p.includes('@') ? ' ionice-alias' : '';
    assert.deepEqual(names(`[Service]\nExecStart=${p}/usr/bin/ionice${argv0} -c3 /x\n`, 'a.service'), [[2, 'ionice -c3']], `prefix "${p}"`);
  }
  assert.deepEqual(names('[Service]\nExecStart=-/usr/bin/ionice -c2 -n7 /x\n', 'a.service'), []);
});

test('C54 more starvable levels: taskpolicy maintenance and throttle, constants, 3 << 13, the thread-priority crate', () => {
  const lines = [
    'Command::new("taskpolicy").args(["-c", "maintenance", exe]);',
    'Command::new("taskpolicy").args(["-d", "throttle", exe]);',
    'const IDLE_FLAGS: u32 = 0x40 | 0x0800_0000;',
    'cmd.creation_flags(IDLE_FLAGS);',
    'const BG: i32 = 0x0001_0000;',
    'unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY(BG)) };',
    'unsafe { libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13) };',
    'set_current_thread_priority(ThreadPriority::Min)?;',
    'set_thread_priority_and_policy(t, p, ThreadSchedulePolicy::Normal(NormalThreadSchedulePolicy::Idle))?;',
    'set_winapi_thread_priority(h, WinAPIThreadPriority::BackgroundModeBegin)?;',
  ];
  assert.deepEqual(names(lines.join('\n')), [
    [1, 'taskpolicy -c maintenance'],
    [2, 'taskpolicy -d throttle'],
    [4, 'IDLE_PRIORITY_CLASS (0x40, through IDLE_FLAGS)'],
    [6, 'THREAD_MODE_BACKGROUND_BEGIN (0x10000, through BG)'],
    [7, 'IOPRIO_CLASS_IDLE (3 << 13)'],
    [8, 'thread-priority ThreadPriority::Min'],
    [9, 'thread-priority NormalThreadSchedulePolicy::Idle'],
    [10, 'thread-priority WinAPIThreadPriority::BackgroundModeBegin'],
  ]);
  assert.deepEqual(rs('const GENTLE: u32 = 0x4000;\ncmd.creation_flags(GENTLE);\nlet x = 2 << 13;\n'), []);
});

test('C54 no false alarms: values with their own keys, test modules, match arms, other commands\' flags', () => {
  // `<true/>` belongs to the key written right before it, not to LowPriorityIO two keys back.
  const render = [
    'fn plist(o: &mut String, spec: &Spec) {',
    '    key(o, "LowPriorityIO");',
    '    string(o, "    ", spec.io());',
    '    key(o, "RunAtLoad");',
    '    o.push_str("    <true/>\\n");',
    '}',
  ].join('\n');
  assert.deepEqual(rs(render), []);
  // Identifiers in a test module (asserting what a level is) are not uses.
  assert.deepEqual(rs('fn f() {}\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { assert_ne!(class(), QOS_CLASS_BACKGROUND); }\n}\n'), []);
  // A match arm that reads a level (decoding what the OS reports) is not a use.
  assert.deepEqual(rs('match class {\n    QOS_CLASS_BACKGROUND => "background",\n    Some(SCHED_IDLE) => "idle",\n    _ => "other",\n}\n'), []);
  assert.deepEqual(rs('match self {\n    ProcessType::Background => "Background",\n}\n'), []);
  // A flag of another statement or another shell command is not this command's.
  assert.deepEqual(rs('Command::new("taskpolicy").args(["-c", "utility", exe]).spawn()?;\nlet ls = ["-b"];\n'), []);
  assert.deepEqual(rs('sh("taskpolicy -c utility make; ls -b");\nsh("chrt --batch 0 make && grep -i x");\n'), []);
});

// ---- cost ------------------------------------------------------------------------------------------------------------

/** The work C54 does on `text`, in steps (each character lexed, each tag, token, line and look-ahead visited). */
const steps = (text, file) => {
  const stats = { steps: 0 };
  starvableUses(text, file, stats);
  return stats.steps;
};

test('C54 work grows linearly: ten times the input takes about ten times the steps, on hostile input too', () => {
  // The old gap regex backtracked exponentially on these (about 90 bytes never finished); the scans count their work, so
  // linear growth is checked by counting, not by a clock.
  const hostile = [
    `<key>ProcessType</key>${'" "'.repeat(20)}`,
    `<key>ProcessType</key>${'"" '.repeat(20)}`,
    `"<key>LowPriorityIO</key>${'" "'.repeat(20)}"`,
    `IOSchedulingClass${' '.repeat(40)}=${' '.repeat(40)}x\n`,
    `"taskpolicy" ${'"-x" '.repeat(20)}\n`,
    'cmd.creation_flags(creation_flags(creation_flags(0x40)));\n',
    'creation_flags(THREAD_PRIORITY(',
    '#[cfg(test)]\nmod t {\n}\n',
    `${'/* /* */ */'.repeat(5)} "\\" r#"x"# 'a' 'b\n`,
  ];
  for (const h of hostile) {
    for (const file of ['src/x.rs', 'a.plist', 'a.service']) {
      const n = steps(h.repeat(10), file);
      const tenN = steps(h.repeat(100), file);
      // Linear work gives about 10 (copies can lex a little differently at their seams); quadratic would give about 100.
      assert.ok(tenN <= 15 * n, `${file}: ${n} steps for 10 copies, ${tenN} for 100, on ${JSON.stringify(h.slice(0, 30))}`);
    }
  }
});
