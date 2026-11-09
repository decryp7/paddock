use std::net::TcpListener;

// The function itself is passed on, never called here.
pub fn serve(addresses: Vec<&str>) {
    let _ = addresses.into_iter().map(TcpListener::bind).next();
}
