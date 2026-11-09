// The file of `#[cfg(test)] mod declared_elsewhere;` in skipped_test_forms.rs: test code.
pub fn helper() {
    let _ = std::net::TcpListener::bind("127.0.0.1:0");
}
