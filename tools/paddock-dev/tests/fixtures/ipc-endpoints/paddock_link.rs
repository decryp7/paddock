use paddock_ipc::link::{LinkServer, ServerConfig};

pub fn serve(config: ServerConfig, handler: std::sync::Arc<dyn paddock_ipc::Handler>) {
    let _ = LinkServer::start(config, handler);
}
