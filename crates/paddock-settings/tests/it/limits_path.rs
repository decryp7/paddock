//! `paddock_limits::SettingPath` names a Setting by its id, so it must accept exactly what `valid_id` accepts.

use paddock_limits::SettingPath;
use paddock_settings::valid_id;

#[test]
fn a_setting_path_has_the_grammar_of_a_setting_id() {
    let long_ok = "a".repeat(128);
    let long_bad = "a".repeat(129);
    let cases = [
        "",
        "a",
        "performance.pluginMemoryMb",
        "a-b_c.D9",
        "a b",
        "a/b",
        "a:b",
        "é",
        ".",
        "-",
        "_",
        "9",
        &long_ok,
        &long_bad,
    ];
    for case in cases {
        assert_eq!(SettingPath::is_valid(case), valid_id(case), "{case:?}");
    }
}
