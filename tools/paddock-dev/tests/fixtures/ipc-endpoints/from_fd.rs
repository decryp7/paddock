use std::os::fd::OwnedFd;

pub fn serve(fd: OwnedFd, other: OwnedFd) {
    let _a = std::net::TcpListener::from(fd);
    let _b = std::os::unix::net::UnixListener::from(other);
}
