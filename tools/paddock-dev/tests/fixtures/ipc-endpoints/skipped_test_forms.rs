// None of these is production code: the scan must report nothing from this file.
use std::net::TcpListener;

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    fn helper() {
        let _ = TcpListener::bind("127.0.0.1:0");
    }
}

#[cfg(all(test, unix))]
mod unix_tests {
    fn helper() {
        let _ = std::os::unix::net::UnixListener::bind("/tmp/x");
    }
}

#[test]
fn a_test() {
    let _ = TcpListener::bind("127.0.0.1:0");
}

#[tokio::test]
async fn an_async_test() {
    let _ = tokio::net::TcpListener::bind("127.0.0.1:0").await;
}

#[cfg(test)]
mod declared_elsewhere;

#[cfg(test)]
fn only_in_tests() {
    let _ = TcpListener::bind("127.0.0.1:0");
}

pub struct Holder;

#[cfg(test)]
impl Holder {
    fn listen_for_tests() {
        let _ = TcpListener::bind("127.0.0.1:0");
    }
}
