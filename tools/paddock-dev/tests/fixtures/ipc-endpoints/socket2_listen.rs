use socket2::{Domain, Socket, Type};

pub fn serve() -> std::io::Result<Socket> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, None)?;
    socket.listen(128)?;
    Ok(socket)
}
