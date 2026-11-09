use std::os::unix::net::{SocketAddr, UnixListener};

pub fn serve(address: &SocketAddr) {
    let _ = UnixListener::bind_addr(address);
}
