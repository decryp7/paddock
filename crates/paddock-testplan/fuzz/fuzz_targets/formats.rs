//! Both parsers on arbitrary bytes: never a panic, and whatever parses writes back to a file that parses to the same value.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_testplan::{NoRedaction, parse_plan, parse_result};

fuzz_target!(|data: &[u8]| {
    if let Ok(plan) = parse_plan(data) {
        let text = plan.to_toml();
        assert_eq!(parse_plan(text.as_bytes()).expect("a written plan parses"), plan);
    }
    if let Ok(result) = parse_result(data) {
        let text = result.to_toml(&NoRedaction);
        assert_eq!(parse_result(text.as_bytes()).expect("a written result parses"), result);
    }
});
