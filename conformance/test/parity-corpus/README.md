# Parity corpus

Text shapes on which the JavaScript host and the Rust host of a check have diverged or could: astral characters and emoji (a
JavaScript regular expression without the `u` flag counts two units), Unicode case folding (`ſ` and the Kelvin sign fold into ASCII
letters in a `u`+`i` match), CRLF, a byte order mark, no-break spaces, the other line separators and very long lines.
`variants.json` lists them; `tools/paddock-dev/tests/it/parity_corpus.rs` applies each to a check's own control tree and compares the
findings of both hosts. C132 flags a regular expression of a dual-host check without the `u` flag.
