// A function called by its bare name inside its own crate (no `use` to resolve).
pub fn serve_loopback() {}

pub fn start() {
    serve_loopback();
}
