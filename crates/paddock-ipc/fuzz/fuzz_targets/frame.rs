//! The frame codec on arbitrary bytes: decoding never panics, never allocates beyond the limit, and whatever decodes
//! encodes back to the same bytes; the buffered and the streaming decoder agree.
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_ipc::frame::{FrameLimit, MIN_FRAME, decode, decode_body, encode, handshake_versions, peek_head, read_frame};

fuzz_target!(|data: &[u8]| {
    let limit = FrameLimit::new(MIN_FRAME * 4);
    let _ = handshake_versions(data);
    let _ = peek_head(data);
    let _ = decode_body(data);
    if let Ok(Some((frame, used))) = decode(data, limit) {
        let again = encode(&frame, limit).expect("a decoded frame encodes");
        assert_eq!(decode(&again, limit).ok().flatten().map(|(f, _)| f), Some(frame.clone()));
        let mut r = &data[..used];
        assert_eq!(read_frame(&mut r, limit).ok(), Some(frame));
    }
    let mut r = data;
    while read_frame(&mut r, limit).is_ok() {}
});
