//! A never-panics check over deterministic hostile input (the proptest-free half of plan X41).

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Hostile variants of `seed`, deterministic and bounded (at most a few hundred, each at most `seed.len() + 16` bytes):
/// empty, every truncation point, single bit flips of the first bytes, runs of `0x00` and `0xff`, a lone `\r`, a NUL in
/// the middle, an invalid UTF-8 byte, the seed doubled and the seed with its last byte repeated.
pub fn corpus(seed: &[u8]) -> Vec<Vec<u8>> {
    let mut out: Vec<Vec<u8>> = vec![Vec::new()];
    for cut in 1..seed.len() {
        out.push(seed[..cut].to_vec());
    }
    for i in 0..seed.len().min(64) {
        for bit in 0..8 {
            let mut v = seed.to_vec();
            v[i] ^= 1 << bit;
            out.push(v);
        }
    }
    let n = seed.len().max(1);
    out.push(vec![0x00; n]);
    out.push(vec![0xff; n]);
    out.push(vec![b'\r'; n.min(16)]);
    if !seed.is_empty() {
        let mid = seed.len() / 2;
        let mut nul = seed.to_vec();
        nul.insert(mid, 0);
        out.push(nul);
        let mut bad = seed.to_vec();
        bad.insert(mid, 0xc3);
        out.push(bad);
        let mut doubled = seed.to_vec();
        doubled.extend_from_slice(seed);
        out.push(doubled);
        let mut repeated = seed.to_vec();
        repeated.extend(std::iter::repeat_n(seed[seed.len() - 1], 16));
        out.push(repeated);
    }
    out
}

/// Runs `parse` on every input and returns the inputs it panicked on (empty: it never panicked). The parser's result
/// (an error for bad input is fine) is ignored. For a test: `assert!(never_panics(&corpus(SEED), parse).is_empty())`.
pub fn never_panics<I: AsRef<[u8]>, R>(
    inputs: &[I],
    mut parse: impl FnMut(&[u8]) -> R,
) -> Vec<Vec<u8>> {
    inputs
        .iter()
        .filter(|input| catch_unwind(AssertUnwindSafe(|| parse(input.as_ref()))).is_err())
        .map(|input| input.as_ref().to_vec())
        .collect()
}

/// The test of a parser in one line: runs `parse` over the [`corpus`] of every seed (and of the empty seed) and panics, naming the
/// first input that made it panic (lossy text, so the message is readable). A parser returning an error for hostile input is fine.
///
/// ```
/// paddock_testkit::assert_never_panics(&[b"key = value\n"], |bytes| String::from_utf8_lossy(bytes).contains('='));
/// ```
pub fn assert_never_panics<R>(seeds: &[&[u8]], mut parse: impl FnMut(&[u8]) -> R) {
    let inputs: Vec<Vec<u8>> = seeds
        .iter()
        .flat_map(|s| corpus(s))
        .chain(corpus(b""))
        .collect();
    let panicked = never_panics(&inputs, &mut parse);
    assert!(
        panicked.is_empty(),
        "the parser panicked on {} of {} input(s), first: {:?}",
        panicked.len(),
        inputs.len(),
        panicked
            .first()
            .map(|b| String::from_utf8_lossy(b).into_owned())
    );
}
