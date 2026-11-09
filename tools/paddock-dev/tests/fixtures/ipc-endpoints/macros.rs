// Listen calls inside macros: arguments that parse as expressions are read as such (vec!, join!); `select!` arms do not parse
// as expressions, so the macro itself is the site (the scan fails closed on a listen identifier among its tokens).
pub fn serve() {
    let _a = vec![std::net::TcpListener::bind("127.0.0.1:0")];
}

pub async fn together() {
    let _ = tokio::join!(tokio::net::TcpListener::bind("127.0.0.1:0"), tokio::net::UnixListener::bind("x"));
}

pub async fn arms() {
    let _ = tokio::select! { listener = tokio::net::TcpListener::bind("127.0.0.1:0") => listener };
}
