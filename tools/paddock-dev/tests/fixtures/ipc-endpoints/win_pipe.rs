use windows_sys::Win32::System::Pipes::CreateNamedPipeW;

pub unsafe fn serve(name: *const u16) {
    unsafe { CreateNamedPipeW(name, 3, 0, 255, 4096, 4096, 0, std::ptr::null()) };
}
