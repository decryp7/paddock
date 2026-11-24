import assert from 'node:assert/strict';
import { test } from 'node:test';
import { writeAtomicSites } from '../src/checks/C169-write-atomic.mjs';

const lines = (text, file = 'crates/a/src/x.rs') => writeAtomicSites(text, file).map((s) => s.line);

test('C169: a hand-written temporary file and rename is flagged (the avatar and the reuse store)', () => {
  assert.deepEqual(lines('fn w(path: &Path, bytes: &[u8]) -> io::Result<()> {\n    let tmp = path.with_extension("tmp");\n    std::fs::write(&tmp, bytes)?;\n    std::fs::rename(&tmp, path)\n}\n'), [4]);
  assert.deepEqual(lines('fn w() {\n    let _ = fs::rename(tmp_path, to);\n}\n'), [2]);
  assert.deepEqual(lines('fn w(&self) {\n    std::fs::rename(&self.tmp, &self.path).ok();\n}\n'), [2]);
  assert.deepEqual(lines('async fn w() {\n    tokio::fs::rename(&temp_file, to).await.ok();\n}\n'), [2]);
  assert.deepEqual(lines('fn w() {\n    std::fs::write(&tmp, b).and_then(|()| std::fs::rename(&tmp, &path))\n}\n'), [2]);
});

test('C169: streaming writers, moves, rotations, tests, paddock-env, comments, strings and waivers are fine', () => {
  assert.deepEqual(lines('fn w() {\n    std::fs::rename(&part, &dest)?;\n    std::fs::rename(staging, dest)?;\n    std::fs::rename(&path, &aside)?;\n}\n'), []);
  assert.deepEqual(lines('fn w() {\n    // std::fs::rename(&tmp, p)\n    let s = "std::fs::rename(&tmp, p)";\n}\n'), []);
  assert.deepEqual(lines('fn w() {\n    // conformance: write-atomic — the rename claims the name, a lock\n    fs::rename(&tmp, &m).ok();\n}\n'), []);
  assert.deepEqual(lines('fn p() {}\n#[cfg(test)]\nmod tests {\n    fn t() {\n        std::fs::rename(&tmp, &p).unwrap();\n    }\n}\n'), [], 'a test module');
  assert.deepEqual(lines('fn w() {\n    std::fs::rename(&tmp, p)?;\n}\n', 'crates/paddock-env/src/atomic.rs'), [], 'the shared implementation');
  assert.deepEqual(lines('fn w() {\n    std::fs::rename(&tmp, p)?;\n}\n', 'crates/a/tests/it/x.rs'), [], 'an integration test');
  assert.deepEqual(lines('fn w() {\n    std::fs::rename(&attempt, p)?;\n    std::fs::rename(&timeline, p)?;\n}\n'), [], 'a name that only contains the letters');
});
