//! The Tauri adapter on Tauri's mock runtime: one shared downloader in the app state.
#![cfg(feature = "tauri")]

use paddock_download::{Downloader, Get, Response, Transport, TransportError, TransportErrorKind};
use std::sync::Arc;
use tauri::test::{mock_builder, mock_context, noop_assets};

struct Offline;

impl Transport for Offline {
    fn get(&self, _: &Get<'_>) -> Result<Response, TransportError> {
        Err(TransportError::new(TransportErrorKind::Connect, "offline"))
    }
}

#[test]
fn manage_shares_one_downloader_and_keeps_the_first() {
    let app = mock_builder().build(mock_context(noop_assets())).unwrap();
    let handle = app.handle();
    assert!(
        paddock_download::tauri::downloader(handle).is_none(),
        "nothing before manage"
    );
    let first = paddock_download::tauri::manage(handle, Downloader::new(Arc::new(Offline)));
    let again = paddock_download::tauri::manage(handle, Downloader::new(Arc::new(Offline)));
    assert!(
        Arc::ptr_eq(&first, &again),
        "a second manage keeps the first downloader"
    );
    let found = paddock_download::tauri::downloader(handle).unwrap();
    assert!(Arc::ptr_eq(&first, &found));
    assert!(
        Arc::ptr_eq(found.hosts(), first.hosts()),
        "the host caps are shared"
    );
}
