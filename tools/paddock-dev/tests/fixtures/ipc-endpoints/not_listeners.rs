// Calls that look like listen primitives and are not: the scan must report nothing from this file.
mod other {
    pub struct Host;
    impl Host {
        pub fn listen(&self) {}
    }
    pub struct TcpListener;
    impl TcpListener {
        pub fn bind(_: &str) {}
    }
}

use other::{Host, TcpListener};

pub fn calls(tauri_window: &Window) {
    TcpListener::bind("not a socket");
    Host.listen();
    // Tauri's event listener has two arguments and no socket2 import in this file.
    tauri_window.listen("event", |_| {});
    let _ = std::fs::File::create("a-file");
}

pub struct Window;
impl Window {
    pub fn listen(&self, _event: &str, _f: impl Fn(u8)) {}
}
