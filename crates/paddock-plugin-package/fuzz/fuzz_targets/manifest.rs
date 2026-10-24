//! The manifest parser on arbitrary text: parsing never panics, and whatever parses is valid (`check` agrees), writes back to
//! TOML that parses to the same manifest, and survives a trip through JSON.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_plugin_package::Manifest;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    if let Ok(manifest) = Manifest::parse(text) {
        manifest.check().expect("a parsed manifest passes its own check");
        let again = Manifest::parse(&manifest.to_toml()).expect("the written manifest parses");
        assert_eq!(again, manifest);
        let json = serde_json::to_string(&manifest).expect("a manifest is JSON");
        assert_eq!(serde_json::from_str::<Manifest>(&json).expect("and reads back"), manifest);
    }
});
