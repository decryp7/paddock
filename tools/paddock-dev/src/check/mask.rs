//! `mask_rust`: Rust source with its comments and the insides of its string, raw-string and char literals blanked (newlines kept; one
//! blank per UTF-16 unit, as JavaScript counts), read off rustc's own lexer (`lex::rust`). C39, C53 and C120 read positions off it. The
//! JavaScript host's `maskRust` (`conformance/src/util/rust-lexer.mjs`) is held to it by the parity tests.

/// `text` with comments and the insides of string, raw-string, byte-string, C-string and char literals blanked (newlines kept). A literal
/// that is not terminated keeps its opener and is blank to the end of the file.
pub fn mask_rust(text: &str) -> String {
    super::lex::rust::mask(text)
}

/// The masked text as UTF-16 units.
pub fn mask_units(t: &[u16]) -> Vec<u16> {
    mask_rust(&String::from_utf16_lossy(t)).encode_utf16().collect()
}
