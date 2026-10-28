import assert from 'node:assert/strict';
import { test } from 'node:test';
import { freedPorts, maskRust } from '../src/checks/C39-freed-port.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const wrap = (body) => `#[test]\nfn it() {\n${body}\n}\n`;
const kinds = (text) => freedPorts(text).map((p) => p.kind);

test('C39 bad: a port read off a listener that is dropped (block, chain, drop, fn)', () => {
  // The shape of the Pitwall link test that failed on Windows CI (run 6512).
  const block = wrap('    let free = {\n        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();\n        l.local_addr().unwrap().port()\n    };\n    probe(free);');
  assert.deepEqual(kinds(block), ['block']);
  assert.equal(freedPorts(block)[0].line, 4);
  assert.deepEqual(kinds(wrap('    let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();')), ['chain']);
  assert.deepEqual(kinds(wrap('    let p = std::net::TcpListener::bind("127.0.0.1:0")\n        .unwrap()\n        .local_addr()\n        .unwrap()\n        .port();')), ['chain']);
  assert.deepEqual(kinds(wrap('    let p = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port();')), ['chain']);
  assert.deepEqual(kinds(wrap('    let a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap().local_addr().unwrap();')), ['chain']);
  assert.deepEqual(kinds('fn free_port() -> u16 {\n    let l = TcpListener::bind("127.0.0.1:0").expect("bind");\n    l.local_addr().unwrap().port()\n}\n'), ['block']);
  assert.deepEqual(kinds(wrap('    let l = TcpListener::bind("127.0.0.1:0").unwrap();\n    let url = format!("http://{}/", l.local_addr().unwrap());\n    drop(l);\n    get(&url);')), ['drop']);
});

test('C39 bad: other unwrap tails, a renamed import and a socket2 socket', () => {
  for (const tail of ['.unwrap_or_else(|e| panic!("{e}"))', '.map_err(Error::Io)?', '.context("bind")?', '.with_context(|| "bind")?']) {
    assert.deepEqual(kinds(wrap(`    let p = TcpListener::bind("127.0.0.1:0")${tail}.local_addr().unwrap().port();`)), ['chain'], tail);
  }
  assert.deepEqual(kinds('use std::net::TcpListener as L;\n' + wrap('    let p = L::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();')), ['chain']);
  const s2 = 'fn free() -> u16 {\n    let s = Socket::new(Domain::IPV4, Type::STREAM, None).unwrap();\n    s.set_reuse_address(true).unwrap();\n    s.bind(&addr.into()).unwrap();\n    s.local_addr().unwrap().as_socket().unwrap().port()\n}\n';
  assert.deepEqual(kinds(s2), ['block']);
});

test('C39 good: a listener kept, served, moved or dropped after its last use', () => {
  // Served by a thread: the listener moves into it.
  assert.deepEqual(freedPorts(wrap('    let l = TcpListener::bind("127.0.0.1:0").unwrap();\n    let port = l.local_addr().unwrap().port();\n    std::thread::spawn(move || { let _ = l.accept(); });\n    get(port);')), []);
  // A block that returns the listener with its port.
  assert.deepEqual(freedPorts('fn serve() -> (TcpListener, u16) {\n    let l = TcpListener::bind("127.0.0.1:0").unwrap();\n    let p = l.local_addr().unwrap().port();\n    (l, p)\n}\n'), []);
  // Dropped once the test no longer uses the port; a later `let` of the same name is another value.
  assert.deepEqual(freedPorts(wrap('    let silent = TcpListener::bind("127.0.0.1:0").unwrap();\n    let mut m = mail(silent.local_addr().unwrap().port());\n    send(&m);\n    drop(silent);\n    let mut m = mail(1);\n    send(&m);')), []);
  assert.deepEqual(freedPorts(wrap('    let held = HeldPort::reset();\n    get(held.port());')), []);
  // Comments, strings, raw strings and char literals never count, and do not confuse the scan.
  assert.deepEqual(freedPorts(wrap('    // let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();\n    let s = r#"TcpListener::bind("x").unwrap().local_addr()"#;\n    let q = \'"\';')), []);
});

test('C39 waivers: with a reason the site passes, without one it is reported', () => {
  const site = '    let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();';
  assert.equal(freedPorts(wrap(`    // conformance: freed-port — the child process binds it and the start retries on a taken port\n${site}`))[0].waived, 'reason');
  assert.equal(freedPorts(wrap(`${site} // conformance: freed-port`))[0].waived, 'no-reason');
});

test('maskRust: char literals and raw strings do not open strings that never end', () => {
  const m = maskRust('let q = \'"\'; let r = r#"a"b"#; let c = \'\\n\'; fn f<\'a>(x: &\'a str) {}\nTcpListener::bind(x)');
  assert.ok(m.includes('TcpListener::bind(x)'));
  assert.ok(m.includes("<'a>"));
  assert.equal(m.split('\n').length, 2);
});

test('C39 through the runner: an error per freed port, none for held or waived ones', async () => {
  const bad = wrap('    let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();\n    get(p);');
  const good = wrap('    let held = paddock_testkit::net::HeldPort::hang_up();\n    get(held.port());');
  const waived = wrap('    // conformance: freed-port — handed to a child that retries\n    let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();');
  const bare = wrap('    // conformance: freed-port\n    let p = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();');
  const f = await findingsOf('C39', makeApp({ 'crates/a/tests/bad.rs': bad, 'crates/a/tests/good.rs': good, 'crates/a/src/w.rs': waived, 'crates/a/src/b.rs': bare }));
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [['crates/a/src/b.rs', 'error'], ['crates/a/tests/bad.rs', 'error']]);
  assert.match(f.find((x) => x.file.endsWith('bad.rs')).fix, /HeldPort/);
  assert.match(f.find((x) => x.file.endsWith('b.rs')).message, /no reason/);
});
