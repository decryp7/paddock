use tokio::net::windows::named_pipe::ServerOptions;

pub fn serve(name: &str, attributes: *mut std::ffi::c_void) {
    let mut options = ServerOptions::new();
    let _a = options.create(name);
    let _b = unsafe { options.create_with_security_attributes_raw(name, attributes) };
}
