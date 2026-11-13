// The 36 probes of the WP-10 batch 2 review, each with the answer the language gives (the Rust host's tokenizers, rustc's lexer,
// tree-sitter-bash and tree-sitter-yaml, are the oracle: tools/paddock-dev/tests/it/check_lex.rs compares the two hosts on the same file).
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { commentView } from '../src/util/inline-waiver.mjs';

const probes = JSON.parse(readFileSync(new URL('../../tools/paddock-dev/tests/fixtures/lex/probes.json', import.meta.url), 'utf8'));

test('the review probes: a marker is a waiver only where the language has a comment', () => {
  assert.equal(probes.length, 36);
  for (const p of probes) {
    const state = commentView(p.text, p.lang).waiver(p.line, p.tag).state;
    assert.equal(state === 'reason' ? 'reason' : 'none', p.expect, p.name);
  }
});
