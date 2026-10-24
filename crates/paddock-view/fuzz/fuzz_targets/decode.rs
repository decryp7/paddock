//! Arbitrary bytes and text through every decoder and the validator: nothing panics or overflows the stack, and a
//! document that decodes and validates encodes back to something that decodes to the same document.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_view::{
    ViewAction, ViewDoc, ViewPatch, apply_patch, from_json, from_postcard, to_json, to_postcard,
    validate, validate_action,
};

fuzz_target!(|data: &[u8]| {
    if let Ok(doc) = from_postcard::<ViewDoc>(data) {
        if validate(&doc).is_ok() {
            let bytes = to_postcard(&doc).expect("a valid document encodes");
            assert_eq!(from_postcard::<ViewDoc>(&bytes).ok(), Some(doc.clone()));
            // The JSON text can be larger than the postcard bytes, and then it is refused by size, not by shape.
            if let Ok(json) = to_json(&doc) {
                assert_eq!(from_json::<ViewDoc>(&json).ok(), Some(doc));
            }
        }
    }
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(doc) = from_json::<ViewDoc>(text) {
            let _ = validate(&doc);
        }
        if let Ok(patch) = from_json::<ViewPatch>(text) {
            let mut doc = ViewDoc {
                schema: 1,
                title: "t".into(),
                root: paddock_view::Node::text("x").with_id("x"),
                etag: 1,
            };
            let _ = apply_patch(&mut doc, patch);
        }
        if let Ok(a) = from_json::<ViewAction>(text) {
            let _ = validate_action(&a);
        }
    }
    if let Ok(patch) = from_postcard::<ViewPatch>(data) {
        let mut doc = ViewDoc {
            schema: 1,
            title: "t".into(),
            root: paddock_view::Node::text("x").with_id("x"),
            etag: 1,
        };
        let _ = apply_patch(&mut doc, patch);
    }
    if let Ok(a) = from_postcard::<ViewAction>(data) {
        let _ = validate_action(&a);
    }
});
