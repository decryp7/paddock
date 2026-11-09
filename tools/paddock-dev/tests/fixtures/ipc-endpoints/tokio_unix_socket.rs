pub async fn serve(socket: tokio::net::UnixSocket) {
    let _listener = socket.listen(1024);
}
