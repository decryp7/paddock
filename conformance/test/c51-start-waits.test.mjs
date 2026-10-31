import assert from 'node:assert/strict';
import { test } from 'node:test';
import { startWaits } from '../src/checks/C51-start-waits.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (t) => startWaits(t).map((s) => s.line);

test('C51 flags a fixed start constant, field and a wait inside a start function', () => {
  assert.deepEqual(lines('pub const START_LIMIT: Duration = Duration::from_secs(20);\n'), [1]);
  assert.deepEqual(lines('struct O {\n    pub spawn_timeout: Duration,\n    pub ready_deadline: Option<Duration>,\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn start_agent() {\n    let end = Deadline::after(clock, d);\n    rx.recv_timeout(d);\n}\nfn other() {\n    rx.recv_timeout(d);\n}\n'), [2, 3]);
  assert.deepEqual(lines('fn launch() {\n    let t = Instant::now() + d;\n}\n'), [2]);
  assert.deepEqual(lines('static SPAWN_WAIT: std::time::Duration = D;\n'), [1]);
});

test('C51 passes waived sites, the judge, other names and test modules', () => {
  assert.deepEqual(lines('// cap: the render allowance a user sets in Settings\npub const START_LIMIT: Duration = D;\n'), []);
  assert.deepEqual(lines('pub const START_LIMIT: Duration = D; // external: the OS decides\n'), []);
  assert.deepEqual(lines('fn start() {\n    // pace: a poll interval, bounds nothing\n    rx.recv_timeout(POLL);\n}\n'), []);
  assert.deepEqual(lines('let p: Patience = Patience::default(); // START_LIMIT: Duration\n'), []);
  assert.deepEqual(lines('const POLL: Duration = D;\nconst CLOSE_GRACE: Duration = D;\npub start_page: String,\n'), []);
  assert.deepEqual(lines('// const START_LIMIT: Duration = D;\nfn a() { let s = "START_LIMIT: Duration"; }\n'), []);
  assert.deepEqual(lines('fn a() {}\n#[cfg(test)]\nmod tests {\n    const START_LIMIT: Duration = D;\n    fn start() { rx.recv_timeout(d); }\n}\n'), []);
  assert.deepEqual(lines('fn start() {\n}\nfn later() {\n    rx.recv_timeout(d);\n}\n'), []);
});

test('C51 sees code after a test module and takes connect functions', () => {
  assert.deepEqual(lines('#[cfg(test)]\nmod tests {\n    const START_LIMIT: Duration = D;\n}\npub const SPAWN_TIMEOUT: Duration = D;\n'), [5]);
  assert.deepEqual(lines('fn pipe_connector() {\n    rx.recv_timeout(d);\n}\nfn start_launchctl_job() {\n    // pace: poll\n    rx.recv_timeout(d);\n}\n'), []);
});

test('C51 does not take a function that merely begins with a start word', () => {
  assert.deepEqual(lines('fn launchctl(d: Duration) {\n    rx.recv_timeout(d);\n}\nfn starting_up() {\n    rx.recv_timeout(d);\n}\n'), []);
  assert.deepEqual(lines('fn launch() {\n    rx.recv_timeout(d);\n}\n'), [2]);
  assert.deepEqual(lines('fn launch_agent() {\n    rx.recv_timeout(d);\n}\n'), [2]);
});

test('C51 through the runner: warnings only, for source in scope only', async () => {
  const bad = 'pub const START_LIMIT: Duration = D;\n';
  const f = await findingsOf(
    'C51',
    makeApp({
      'crates/a/src/lib.rs': bad,
      'apps/x/crates/y/src/lib.rs': bad,
      'apps/x/app/src-tauri/src/l.rs': bad,
      'tools/t/src/main.rs': bad,
      'crates/a/tests/t.rs': bad,
      'crates/a/examples/e.rs': bad,
      'ui/other.rs': bad,
      'crates/a/src/ok.rs': '// cap: product rule\nconst START_LIMIT: Duration = D;\n',
    }),
  );
  assert.deepEqual(f.map((x) => x.file).sort(), ['apps/x/app/src-tauri/src/l.rs', 'apps/x/crates/y/src/lib.rs', 'crates/a/src/lib.rs', 'tools/t/src/main.rs']);
  assert.ok(f.every((x) => x.severity === 'warn' && x.rule === 40 && /StartJudge/.test(x.fix)));
});

const OLD_CONNECT = `fn connect(dir: &Path, process: &mut Supervised) -> Result<Connection> {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(s) = std::fs::read_to_string(dir.join("DevToolsActivePort")) {
            return Connection::connect(1, s.trim());
        }
        if !process.is_running() {
            return Err(Error::Launch("exited".into()));
        }
        if Instant::now() > end {
            return Err(Error::Timeout("the browser start"));
        }
    }
}
`;

test('C51 flags the old Debrief connect() body: a fixed 20 s for DevToolsActivePort', () => {
  assert.deepEqual(lines(OLD_CONNECT), [2, 11]);
});

test('C51 flags the shape in a function whose name says nothing about a start', () => {
  const body = OLD_CONNECT.replace('fn connect(', 'fn attach_to_browser(');
  assert.deepEqual(lines(body), [2, 11]);
  const limit = 'fn attach() {\n    let limit = Duration::from_secs(30);\n    let t = Instant::now();\n    while t.elapsed() < limit {\n        if child.try_wait().is_ok() {}\n    }\n}\n';
  assert.deepEqual(lines(limit), [2, 4]);
  assert.deepEqual(lines('fn f() {\n    let n = Instant::now();\n    loop {\n        if kill(pid, 0) != 0 { break; }\n        if n.elapsed() > D { break; }\n    }\n}\n'), [5]);
});

test('C51 passes the fixed start-judge form', () => {
  const fixed = `fn connect(dir: &Path, process: &mut Supervised, opts: &LaunchOptions) -> Result<Connection> {
    let probe = BrowserProbe { dir, process, conn: None };
    let (outcome, probe) = judge_start(opts.patience, probe, Ticks);
    match outcome {
        Ok(()) => probe.conn.ok_or(Error::Closed),
        Err(StartFailure::Stuck(report)) => Err(Error::Launch(report.message)),
        Err(_) => Err(Error::Closed),
    }
}
`;
  assert.deepEqual(lines(fixed), []);
  assert.deepEqual(lines(OLD_CONNECT.replace('fn connect(', 'fn attach(').replace('let end', 'let end /* Patience */').replace('loop {', 'loop { let _p: Patience;')), [11]);
});

test('C51 flags the WAIT_FOR_OTHER shape: a deadline over a condvar wait', () => {
  const wait = `fn wait_settled(&self) {
    let end = Instant::now() + WAIT_FOR_OTHER;
    while launching() {
        let left = end.checked_duration_since(Instant::now()).ok_or(Timeout)?;
        slots = self.changed.wait_timeout(slots, left).unwrap().0;
    }
}
`;
  assert.deepEqual(lines(wait), [2]);
  assert.deepEqual(lines(wait.replace('fn wait_settled', 'fn settle')), [2]);
});

test('C51 false-positive guards: network deadlines, hang-guards, waivers, test modules', () => {
  const http = 'fn fetch(url: &str) {\n    let deadline = Instant::now() + Duration::from_secs(10);\n    loop {\n        let n = body.read(&mut buf);\n        if Instant::now() > deadline { break; }\n    }\n}\n';
  assert.deepEqual(lines(http), []);
  const timeout = 'fn fetch() {\n    let timeout = Duration::from_secs(10);\n    client.get(url).timeout(timeout).send();\n}\n';
  assert.deepEqual(lines(timeout), []);
  assert.deepEqual(lines(OLD_CONNECT.replace('    let end', '    // hang-guard: test only\n    let end').replace('Error::Timeout("the browser start")', 'E')), []);
  assert.deepEqual(lines(OLD_CONNECT.replace('    let end', '    // cap: a user setting\n    let end').replace('Error::Timeout("the browser start")', 'E')), []);
  assert.deepEqual(lines('fn f() { return Err(Error::Timeout("the request")); }\n'), []);
  assert.deepEqual(lines('fn f() { return Err(Error::Timeout("the browser start")); } // external: the OS\n'), []);
  assert.deepEqual(lines('#[cfg(test)]\nmod tests {\n' + OLD_CONNECT + '}\n'), []);
});

test('C51 flags Timeout string literals that name a start', () => {
  assert.deepEqual(lines('fn f() {\n    return Err(Error::Timeout("the service start"));\n}\n'), [2]);
  assert.deepEqual(lines('fn f() {\n    // Error::Timeout("start")\n}\n'), []);
});

test('C51 name gate takes connect, handshake, wait_for, ready and await; readiness by connect alone', () => {
  for (const n of ['connect', 'handshake_with', 'wait_for_port', 'ready', 'await_up']) {
    assert.deepEqual(lines(`fn ${n}() {\n    rx.recv_timeout(d);\n}\n`), [2], n);
  }
  assert.deepEqual(lines('fn dial() {\n    let end = Instant::now() + D;\n    loop {\n        if TcpStream::connect(addr).is_ok() { break; }\n    }\n}\n'), [2]);
});

test('C51 skips test-only files and does not take a network "connecting" timeout', async () => {
  const bad = 'pub const START_LIMIT: Duration = D;\n';
  const f = await findingsOf('C51', makeApp({ 'crates/a/src/tests.rs': bad, 'crates/a/src/testing.rs': bad, 'crates/a/src/lib.rs': bad }));
  assert.deepEqual(f.map((x) => x.file), ['crates/a/src/lib.rs']);
  assert.deepEqual(lines('fn f() { HttpError::Timeout("connecting") }\n'), []);
});

const POLL = (name, extra = '') =>
  `fn ${name}() {\n    let end = Instant::now() + D;\n    loop {\n        if child.try_wait().is_ok() { break; }${extra}\n        if Instant::now() > end { break; }\n    }\n}\n`;

test('C51 narrowing: stop/run/kill/close helpers, kill-on-timeout, cdp, tests and bins are caps, not start waits (a kill does not exempt)', () => {
  assert.deepEqual(lines(POLL('attach')), [2]);
  for (const n of ['run', 'run_briefly', 'kill_tree', 'close_gracefully', 'terminate', 'shutdown', 'stop_all', 'quit', 'wait_closed', 'wait_event', 'wait_timeout', 'wait_until_for', 'flush']) {
    assert.deepEqual(lines(POLL(n)), [], n);
  }
  assert.deepEqual(lines(`#[test]\n${POLL('attach')}`), []);
  assert.deepEqual(startWaits(POLL('attach'), 'crates/x/src/cdp.rs'), []);
  assert.deepEqual(lines('fn attach() {\n    loop {\n        if started.elapsed() >= cfg.limit { break; }\n        work();\n    }\n}\n'), []);
  assert.deepEqual(lines('fn attach() {\n    let next_start = Instant::now() + PAUSE;\n    loop { child.try_wait(); }\n}\n'), []);
});

test('C51 narrowing keeps the real shapes', () => {
  assert.deepEqual(lines(POLL('connect')), [2]);
  assert.deepEqual(lines('fn attach() {\n    loop {\n        if child.try_wait().is_ok() { break; }\n        if started.elapsed() >= LIMIT { break; }\n    }\n}\n'), [4]);
});

test('C51 flags a start wait that kills its child on timeout, and a kill_on_drop spawn', () => {
  assert.deepEqual(lines(POLL('attach', '\n        child.kill();')), [2]);
  assert.deepEqual(lines(POLL('load_manifests', '\n        kill_tree(child);')), [2]);
  const drop = 'fn spawn_worker() {\n    let c = Command::new(x).kill_on_drop(true).spawn();\n    let end = Instant::now() + D;\n    loop {\n        if c.try_wait().is_ok() { break; }\n    }\n}\n';
  assert.deepEqual(lines(drop), [3]);
  assert.deepEqual(lines(POLL('attach', '\n        child.kill();').replace('    let end', '    // cap: a run limit the caller sets\n    let end')), []);
});

test('C51 elapsed-only: a progress throttle far from the liveness check is not a deadline', () => {
  const far = (gap) => `fn attach() {\n    loop {\n        if child.try_wait().is_ok() { break; }\n${'        work();\n'.repeat(gap)}        if last.elapsed() >= TICK { report(); }\n    }\n}\n`;
  assert.deepEqual(lines(far(8)), []);
  assert.deepEqual(lines(far(1)), [5]);
});

test('C51 does not count the fn line itself as a readiness signal, and blanks an inner fn', () => {
  assert.deepEqual(lines('fn attach(x: T = p.is_running()) {\n    let end = Instant::now() + D;\n    loop { if Instant::now() > end { break; } }\n}\n'), []);
  const outer = 'fn attach() {\n    let end = Instant::now() + D;\n    loop { if Instant::now() > end { break; } }\n    fn inner() { child.try_wait(); }\n}\n';
  assert.deepEqual(lines(outer), []);
  assert.deepEqual(lines('fn attach() { let e = Instant::now() + D; loop { child.try_wait(); } }\n'), [1]);
});

test('C51 skips test items first: #[test] fns and cfg(all(test, ..)) modules', () => {
  assert.deepEqual(lines('#[test]\nfn start_it() {\n    rx.recv_timeout(d);\n}\n'), []);
  assert.deepEqual(lines('#[cfg(all(test, unix))]\nmod tests {\n    const START_LIMIT: Duration = D;\n}\n'), []);
  assert.deepEqual(lines('#[cfg(test)]\nfn helper() {\n    let e = START_LIMIT_X;\n}\npub const SPAWN_WAIT: Duration = D;\n'), [5]);
});

test('C51 skips only fake and fixture binaries in src/bin', async () => {
  const bad = 'pub const START_LIMIT: Duration = D;\n';
  const f = await findingsOf('C51', makeApp({ 'crates/a/src/bin/fake_server.rs': bad, 'crates/a/src/bin/fixture_x.rs': bad, 'crates/a/src/bin/test_helper.rs': bad, 'crates/a/src/bin/tool.rs': bad }));
  assert.deepEqual(f.map((x) => x.file), ['crates/a/src/bin/tool.rs']);
});

test('C51 nested block comments, multi-line and format! timeout literals', () => {
  assert.deepEqual(lines('/* a /* b */ pub const START_LIMIT: Duration = D; */\npub const SPAWN_WAIT: Duration = D;\n'), [2]);
  assert.deepEqual(lines('fn f() {\n    return Err(Error::Timeout(\n        "the browser start",\n    ));\n}\n'), [2]);
  assert.deepEqual(lines('fn f() {\n    Err(Error::Timeout(format!("the {} launch", n)))\n}\n'), [2]);
  assert.deepEqual(lines('fn f() {\n    Err(Error::Timeout(format!("the {} request", n)))\n}\n'), []);
  assert.deepEqual(lines('// Error::Timeout("start")\nfn f() {}\n'), []);
});

test('C51 ignores match arms and comparisons that merely mention a Duration', () => {
  const arm = 'fn attach() {\n    let r = match k {\n        A => Duration::from_secs(1),\n        _ => D,\n    };\n    let ok = limit == Duration::from_secs(1);\n    loop { child.try_wait(); }\n}\n';
  assert.deepEqual(lines(arm), []);
  assert.deepEqual(lines('fn attach() {\n    let end = Duration::from_secs(1);\n    loop { child.try_wait(); }\n}\n'), [2]);
});

test('C51 takes a waiver anywhere in the comment block directly above', () => {
  const body = (c) => `fn f() {\n${c}    let end = Instant::now() + D;\n    loop { child.try_wait(); }\n}\n`;
  assert.deepEqual(lines(body('')), [2]);
  assert.deepEqual(lines(body('    // cap: the grace a child gets;\n    // more words about it\n    // and more\n')), []);
  assert.deepEqual(lines(body('    // cap: the grace\n\n')), [4]);
  assert.deepEqual(lines(body('    // just a note\n')), [3]);
});

test('C51 uses the shared test-item blanking: #[tokio::test], cfg(test) impl', () => {
  assert.deepEqual(lines('#[tokio::test]\nasync fn start_it() {\n    rx.recv_timeout(d);\n}\n'), []);
  assert.deepEqual(lines('#[cfg(test)]\nimpl Foo {\n    fn start() { rx.recv_timeout(d); }\n}\nconst SPAWN_WAIT: Duration = D;\n'), [5]);
});
