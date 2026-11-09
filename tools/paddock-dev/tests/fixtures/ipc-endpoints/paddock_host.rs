use paddock_ipc::Host;

pub fn serve(dir: &std::path::Path, access: &paddock_ipc::Access, token: paddock_ipc::Token) {
    let limit = paddock_ipc::FrameLimit::default();
    let protocol = paddock_ipc::ProtocolVersion::UNVERSIONED;
    let _a = Host::listen(limit, protocol);
    let _b = Host::listen_in(dir, limit, protocol);
    let _c = Host::listen_at("name", access, token, limit, protocol);
}
