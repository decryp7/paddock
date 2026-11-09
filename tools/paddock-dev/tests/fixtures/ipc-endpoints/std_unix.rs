pub fn serve(path: &std::path::Path) -> std::io::Result<std::os::unix::net::UnixListener> {
    std::os::unix::net::UnixListener::bind(path)
}
