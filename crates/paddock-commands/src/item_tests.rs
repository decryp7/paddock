//! Tests of [`crate::item`]: the wire shape, the conversions, the per-item runner and the batch bound.

#![allow(clippy::result_large_err)]

use crate::item::{AtomicReply, BulkReply, ItemResult, check_batch, per_item};
use crate::{CommandError, ErrorCode};
use serde_json::json;

#[test]
fn an_item_is_ok_or_err_on_the_wire() {
    let ok: ItemResult<u32> = ItemResult::Ok(7);
    assert_eq!(serde_json::to_value(&ok).unwrap(), json!({ "ok": 7 }));
    let err: ItemResult<u32> = ItemResult::Err(CommandError::not_found("model gone").field("id"));
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({ "err": { "code": "notFound", "message": "model gone", "retryable": false, "field": "id" } })
    );
    // A unit value (a delete) is `{ ok: null }`.
    assert_eq!(
        serde_json::to_value(ItemResult::Ok(())).unwrap(),
        json!({ "ok": null })
    );
}

#[test]
fn an_item_reads_back_from_the_wire() {
    let back: Vec<ItemResult<String>> =
        serde_json::from_value(json!([{ "ok": "a" }, { "err": { "code": "busy", "message": "later", "retryable": true } }])).unwrap();
    assert_eq!(back[0], ItemResult::Ok("a".to_string()));
    assert_eq!(back[1], ItemResult::Err(CommandError::busy("later")));
    // Neither key, both keys, an unknown code: refused, never guessed.
    assert!(serde_json::from_value::<ItemResult<u8>>(json!({})).is_err());
    assert!(
        serde_json::from_value::<ItemResult<u8>>(
            json!({ "ok": 1, "err": { "code": "io", "message": "x", "retryable": true } })
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<ItemResult<u8>>(
            json!({ "err": { "code": "nope", "message": "x", "retryable": true } })
        )
        .is_err()
    );
}

#[test]
fn a_result_converts_both_ways() {
    let from_ok: ItemResult<u8> = Ok::<u8, std::io::Error>(1).into();
    assert!(from_ok.is_ok());
    let from_err: ItemResult<u8> =
        Err::<u8, _>(std::io::Error::from(std::io::ErrorKind::NotFound)).into();
    assert_eq!(
        from_err.as_ref().err().map(|e| e.code),
        Some(ErrorCode::NotFound)
    );
    assert_eq!(
        from_err.into_result().unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(ItemResult::Ok(3u8).into_result().unwrap(), 3);
    assert_eq!(ItemResult::Ok(3u8).map(|v| v * 2), ItemResult::Ok(6));
}

#[test]
fn per_item_keeps_the_order_and_one_bad_item_fails_only_itself() {
    let out = per_item([1, 0, 4], |n: i32| {
        if n == 0 {
            Err(CommandError::invalid("zero"))
        } else {
            Ok(12 / n)
        }
    });
    assert_eq!(
        out,
        vec![
            ItemResult::Ok(12),
            ItemResult::Err(CommandError::invalid("zero")),
            ItemResult::Ok(3)
        ]
    );
    assert!(per_item(Vec::<i32>::new(), Ok::<i32, CommandError>).is_empty());
}

#[test]
fn a_batch_over_its_bound_is_refused_as_a_whole() {
    assert!(check_batch(0, 3, "ids").is_ok());
    assert!(check_batch(3, 3, "ids").is_ok());
    let e = check_batch(4, 3, "ids").unwrap_err();
    assert_eq!(e.code, ErrorCode::Limit);
    assert_eq!(e.field.as_deref(), Some("ids"));
    assert!(!e.retryable, "the same batch is refused again");
    assert!(
        e.message.contains('4') && e.message.contains('3'),
        "{}",
        e.message
    );
}

#[test]
fn the_two_reply_shapes_say_which_errors_are_whole_batch() {
    // Non-atomic: the outer error is only for the whole call (the bound, a refusal); items fail on their own.
    let items = vec![ItemResult::Ok(1), ItemResult::Err(CommandError::io("disk"))];
    let bulk: BulkReply<u8> = Ok(items.clone());
    assert_eq!(bulk.as_deref(), Ok(&items[..]));
    assert_eq!(
        serde_json::to_value(&items).unwrap(),
        json!([{ "ok": 1 }, { "err": { "code": "io", "message": "disk", "retryable": true } }])
    );
    // Atomic: all or nothing, no per-item errors.
    let atomic: AtomicReply<u8> = Err(CommandError::failed("rolled back"));
    assert!(atomic.is_err());
}
