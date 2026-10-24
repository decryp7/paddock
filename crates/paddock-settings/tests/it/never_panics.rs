//! Stored settings are edited by hand and arrive from the webview: no value makes validation panic (conformance check C36).
use paddock_settings::{
    FieldKind, SectionOwner, SettingField, SettingsSection, sanitize, validate,
};
use paddock_testkit::assert_never_panics;

fn section() -> SettingsSection {
    SettingsSection::new("sd.s", "S", SectionOwner::App)
        .field(SettingField::new("on", "On", FieldKind::Bool, true))
        .field(SettingField::new(
            "n",
            "N",
            FieldKind::Int {
                min: 0,
                max: 9,
                step: 1,
                unit: None,
            },
            3,
        ))
        .field(SettingField::new(
            "t",
            "T",
            FieldKind::Text {
                max_len: 16,
                pattern: Some("[a-z]+".into()),
            },
            "abc",
        ))
}

#[test]
fn values_survive_hostile_json() {
    let section = section();
    assert_never_panics(
        &[
            br#"{"on":false,"n":7,"t":"xyz","extra":[1,2,{"a":null}]}"#,
            b"[1,2,3]",
            b"\"text\"",
        ],
        |b| {
            if let Ok(value) = serde_json::from_slice::<serde_json::Value>(b) {
                let _ = validate(&section, &value);
                let _ = sanitize(&section, &value);
            }
        },
    );
}
