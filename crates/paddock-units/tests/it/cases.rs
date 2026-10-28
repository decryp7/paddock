//! The table shared with the TypeScript twin (`@decryp7/paddock-ui/format`): both sides must produce exactly these texts.

use paddock_units::{Kind, format_bytes, format_whole, parse_size};
use serde_json::Value;

fn table() -> Value {
    serde_json::from_str(include_str!("../../cases.json")).expect("cases.json is JSON")
}

fn run(key: &str, f: &dyn Fn(&Value) -> Value) {
    let table = table();
    let cases = table[key].as_array().unwrap_or_else(|| panic!("{key} cases"));
    assert!(!cases.is_empty(), "{key}");
    for c in cases {
        assert_eq!(f(&c["in"]), c["out"], "{key}({})", c["in"]);
    }
}

fn num(v: &Value) -> u64 {
    v.as_u64().expect("a byte count")
}

fn text(v: &Value) -> &str {
    v.as_str().expect("a text")
}

#[test]
fn formats_storage_and_memory_sizes() {
    run("storage", &|v| format_bytes(num(v), Kind::Storage).into());
    run("memory", &|v| format_bytes(num(v), Kind::Memory).into());
}

#[test]
fn writes_limits_as_the_largest_whole_unit() {
    run("wholeStorage", &|v| format_whole(num(v), Kind::Storage).into());
    run("wholeMemory", &|v| format_whole(num(v), Kind::Memory).into());
}

#[test]
fn reads_typed_sizes() {
    run("parseStorage", &|v| parse_size(text(v), Kind::Storage).expect("a size").into());
    run("parseMemory", &|v| parse_size(text(v), Kind::Memory).expect("a size").into());
}

#[test]
fn refuses_what_is_not_a_size_and_says_what_to_write() {
    let table = table();
    for bad in table["parseBad"].as_array().expect("parseBad") {
        for kind in [Kind::Storage, Kind::Memory] {
            let e = parse_size(text(bad), kind).expect_err(text(bad));
            assert!(e.contains("is not a size"), "{bad}: {e}");
        }
    }
}

#[test]
fn units_are_powers_of_the_step() {
    assert_eq!(Kind::Storage.unit(3), 1_000_000_000);
    assert_eq!(Kind::Memory.unit(3), 1 << 30);
    assert_eq!(paddock_units::exact_bytes(1_234_567), "1,234,567 bytes");
    assert_eq!(paddock_units::exact_bytes(1), "1 byte");
}
