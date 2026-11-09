pub fn serve(accepted: paddock_ipc::Accepted) {
    let _ = accepted.serve_without_handshake("the OS identifies the caller");
}
