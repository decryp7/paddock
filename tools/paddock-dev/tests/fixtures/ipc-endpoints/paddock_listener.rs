use paddock_ipc::endpoint::Listener;

pub fn serve(dir: &std::path::Path, access: &paddock_ipc::Access) {
    let _a = Listener::bind();
    let _b = Listener::bind_in(dir);
    let _c = Listener::bind_fixed("name", access);
    let _d = Listener::bind_system("name", access);
}
