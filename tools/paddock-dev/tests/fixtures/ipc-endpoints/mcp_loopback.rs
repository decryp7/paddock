use paddock_mcp::serve_loopback;

pub fn serve(server: std::sync::Arc<paddock_mcp::Server>, keys: std::sync::Arc<dyn paddock_mcp::KeyVerifier>, l: std::net::TcpListener) {
    let _ = serve_loopback(server, keys, l, 4);
}
