use std::net as network;
use std::net::TcpListener as Listening;

pub fn serve() {
    let _a = Listening::bind("127.0.0.1:0");
    let _b = network::TcpListener::bind("127.0.0.1:0");
}
