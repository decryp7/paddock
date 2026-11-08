//! The bodies that cross the plugin boundary, decoded from arbitrary bytes the way both ends decode them
//! (`paddock_ipc::wire::decode`, postcard): `PlatformRequest` (what the app asks), `HostCall` (what a plugin asks back)
//! and a sample app contract body. Decoding never panics, and what decodes encodes and decodes to the same value
//! (IP1 item 6; the plugin is untrusted, so the app decodes its `HostCall`s and this side's `serve` decodes the app's).
#![no_main]

use libfuzzer_sys::fuzz_target;
use paddock_ipc::wire::{decode, encode};
use paddock_plugin_api::{HostCall, PlatformRequest};
use serde::{Deserialize, Serialize};

/// A contract body of the shape an app defines (enum of structs, strings, vectors, options).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum SampleRequest {
    Ping,
    Echo(String),
    Batch { ids: Vec<u64>, label: Option<String>, nested: Vec<(String, Vec<u8>)> },
}

fn roundtrips<T>(data: &[u8])
where
    T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
{
    if let Ok(value) = decode::<T>(data) {
        let bytes = encode(&value).expect("a decoded value encodes");
        assert_eq!(decode::<T>(&bytes).ok(), Some(value));
    }
}

fuzz_target!(|data: &[u8]| {
    roundtrips::<PlatformRequest>(data);
    roundtrips::<HostCall>(data);
    roundtrips::<SampleRequest>(data);
});
