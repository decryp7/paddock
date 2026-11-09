use tokio::net::*;

pub async fn serve() {
    let _ = TcpListener::bind("127.0.0.1:0").await;
}
