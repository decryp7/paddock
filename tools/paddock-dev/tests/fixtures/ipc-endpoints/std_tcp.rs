use std::net::TcpListener;

pub fn serve() -> std::io::Result<TcpListener> {
    TcpListener::bind("127.0.0.1:0")
}
