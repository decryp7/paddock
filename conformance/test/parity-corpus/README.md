# Parity corpus

Text shapes on which the JavaScript host and the Rust host of a check have diverged or could: astral characters and emoji (a
JavaScript regular expression without the `u` flag counts two units), Unicode case folding (`ſ` and the Kelvin sign fold into ASCII
letters in a `u`+`i` match), CRLF, a byte order mark, no-break spaces, the other line separators and very long lines.
`variants.json` lists them; `tools/paddock-dev/tests/it/parity_corpus.rs` applies each to a check's own control tree and compares the
findings of both hosts.
`u` alone does not make the hosts agree: `\w`, `\b`, `\d`, `\s`, `trim()` and line handling differ even with it, so the corpus, not a lint on flags,
is the mechanism (ESLint's `require-unicode-regexp` is the standard rule if conformance ever gets ESLint).
