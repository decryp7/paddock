use tokio::net::{TcpListener, UnixListener};

pub async fn serve(path: &std::path::Path, std_tcp: std::net::TcpListener, std_unix: std::os::unix::net::UnixListener) {
    let _a = TcpListener::bind("127.0.0.1:0").await;
    let _b = UnixListener::bind(path);
    let _c = TcpListener::from_std(std_tcp);
    let _d = UnixListener::from_std(std_unix);
}
