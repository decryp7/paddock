//! The OS calls, and nothing else: one small adapter per OS behind [`PowerOs`] and [`SleepWakeSource`]. No logic lives here;
//! it is left out of the coverage job (`native.rs`, like the other crates' adapters) and runs only in VMs (ER-14).
#![allow(unsafe_code)]

use crate::{PowerOs, SleepWakeSource};

/// The real keep-awake adapter of this OS. Never use it in a test that runs on a developer's or CI machine (ER-14).
pub fn power() -> Box<dyn PowerOs> {
    Box::new(imp::System)
}

/// The real sleep and wake notifications of this OS. Never use it in a test that runs on a developer's or CI machine (ER-14).
pub fn events() -> Box<dyn SleepWakeSource> {
    Box::new(imp::System)
}

#[cfg(target_os = "macos")]
mod imp {
    use crate::{
        EventSink, Hold, HoldRequest, PowerError, PowerEvent, PowerOs, Scope, SleepWakeSource,
        Subscription,
    };
    use std::ffi::{CString, c_char, c_void};
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::sync::{Arc, mpsc};
    use std::thread::JoinHandle;

    type CfRef = *const c_void;
    const UTF8: u32 = 0x0800_0100;
    const LEVEL_ON: u32 = 255;
    const MSG_CAN_SLEEP: u32 = 0xE000_0270;
    const MSG_WILL_SLEEP: u32 = 0xE000_0280;
    const MSG_HAS_POWERED_ON: u32 = 0xE000_0300;

    #[link(name = "IOKit", kind = "framework")]
    unsafe extern "C" {
        fn IOPMAssertionCreateWithName(kind: CfRef, level: u32, name: CfRef, id: *mut u32) -> i32;
        fn IOPMAssertionRelease(id: u32) -> i32;
        fn IORegisterForSystemPower(
            refcon: *mut c_void,
            port: *mut *mut c_void,
            callback: extern "C" fn(*mut c_void, u32, u32, *mut c_void),
            notifier: *mut u32,
        ) -> u32;
        fn IODeregisterForSystemPower(notifier: *mut u32) -> i32;
        fn IOAllowPowerChange(kernel_port: u32, notification_id: isize) -> i32;
        fn IOServiceClose(connect: u32) -> i32;
        fn IONotificationPortGetRunLoopSource(port: *mut c_void) -> CfRef;
        fn IONotificationPortDestroy(port: *mut c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFRunLoopCommonModes: CfRef;
        static kCFRunLoopDefaultMode: CfRef;
        fn CFStringCreateWithCString(alloc: CfRef, text: *const c_char, encoding: u32) -> CfRef;
        fn CFRelease(object: CfRef);
        fn CFRunLoopGetCurrent() -> CfRef;
        fn CFRunLoopAddSource(run_loop: CfRef, source: CfRef, mode: CfRef);
        fn CFRunLoopRunInMode(mode: CfRef, seconds: f64, return_after_source: u8) -> i32;
        fn CFRunLoopStop(run_loop: CfRef);
    }

    pub struct System;

    /// A `CFString` made from text; released on drop.
    struct CfString(CfRef);

    impl CfString {
        fn new(text: &str) -> Result<Self, PowerError> {
            let c = CString::new(text.replace('\0', " "))
                .map_err(|_| PowerError::Invalid("text with a NUL byte".into()))?;
            // SAFETY: a NUL-terminated UTF-8 string and the default allocator (null).
            let s = unsafe { CFStringCreateWithCString(std::ptr::null(), c.as_ptr(), UTF8) };
            if s.is_null() {
                return Err(PowerError::Os {
                    call: "CFStringCreateWithCString",
                    code: 0,
                    detail: "returned null".into(),
                });
            }
            Ok(Self(s))
        }
    }

    impl Drop for CfString {
        fn drop(&mut self) {
            // SAFETY: the string was created by CFStringCreateWithCString and is released once.
            unsafe { CFRelease(self.0) };
        }
    }

    struct Assertions(Vec<u32>);

    impl Hold for Assertions {
        fn os_ref(&self) -> String {
            let ids: Vec<String> = self.0.iter().map(u32::to_string).collect();
            format!("IOPMAssertion {}", ids.join(","))
        }
    }

    impl Drop for Assertions {
        fn drop(&mut self) {
            for id in &self.0 {
                // SAFETY: the id came from IOPMAssertionCreateWithName and is released once.
                unsafe { IOPMAssertionRelease(*id) };
            }
        }
    }

    fn assertion(kind: &str, name: &str) -> Result<u32, PowerError> {
        let kind = CfString::new(kind)?;
        let name = CfString::new(name)?;
        let mut id = 0u32;
        // SAFETY: both strings are live CFStrings and `id` is a valid out pointer.
        let rc = unsafe { IOPMAssertionCreateWithName(kind.0, LEVEL_ON, name.0, &raw mut id) };
        if rc != 0 {
            return Err(PowerError::Os {
                call: "IOPMAssertionCreateWithName",
                code: i64::from(rc),
                detail: "IOReturn".into(),
            });
        }
        Ok(id)
    }

    impl PowerOs for System {
        fn acquire(&self, request: &HoldRequest) -> Result<Box<dyn Hold>, PowerError> {
            let name = format!("{}: {}", request.who, request.why);
            let mut held = Assertions(Vec::new());
            held.0.push(assertion("PreventUserIdleSystemSleep", &name)?);
            if request.scope == Scope::SystemAndDisplay {
                // On failure `held` drops and releases the first assertion: nothing stays held.
                held.0
                    .push(assertion("PreventUserIdleDisplaySleep", &name)?);
            }
            Ok(Box::new(held))
        }
    }

    struct Context {
        sink: EventSink,
        root: AtomicU32,
    }

    extern "C" fn on_power(
        refcon: *mut c_void,
        _service: u32,
        message: u32,
        argument: *mut c_void,
    ) {
        // SAFETY: refcon is the Context the subscribing thread keeps alive until after the deregistration.
        let ctx = unsafe { &*refcon.cast::<Context>() };
        match message {
            MSG_CAN_SLEEP | MSG_WILL_SLEEP => {
                if message == MSG_WILL_SLEEP {
                    (ctx.sink)(PowerEvent::Sleeping);
                }
                // SAFETY: the root port and the notification id are the ones the OS just gave this callback.
                unsafe { IOAllowPowerChange(ctx.root.load(Ordering::SeqCst), argument as isize) };
            }
            MSG_HAS_POWERED_ON => (ctx.sink)(PowerEvent::Woke),
            _ => {}
        }
    }

    struct Watch {
        stop: Arc<AtomicBool>,
        run_loop: usize,
        thread: Option<JoinHandle<()>>,
    }

    impl Subscription for Watch {}

    impl Drop for Watch {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::SeqCst);
            // SAFETY: CFRunLoopStop is thread-safe; the run loop belongs to the thread joined just below.
            unsafe { CFRunLoopStop(self.run_loop as CfRef) };
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl SleepWakeSource for System {
        fn subscribe(&self, sink: EventSink) -> Result<Box<dyn Subscription>, PowerError> {
            let stop = Arc::new(AtomicBool::new(false));
            let (tx, rx) = mpsc::channel::<Result<usize, PowerError>>();
            let stop_thread = stop.clone();
            let thread = std::thread::Builder::new()
                .name("paddock-power-events".into())
                .spawn(move || {
                    let ctx = Box::new(Context {
                        sink,
                        root: AtomicU32::new(0),
                    });
                    let mut port: *mut c_void = std::ptr::null_mut();
                    let mut notifier = 0u32;
                    let refcon = std::ptr::from_ref::<Context>(&ctx)
                        .cast_mut()
                        .cast::<c_void>();
                    // SAFETY: out pointers are valid, the context outlives the registration (deregistered below).
                    let root = unsafe {
                        IORegisterForSystemPower(refcon, &raw mut port, on_power, &raw mut notifier)
                    };
                    if root == 0 {
                        let _ = tx.send(Err(PowerError::Os {
                            call: "IORegisterForSystemPower",
                            code: 0,
                            detail: "returned no connection".into(),
                        }));
                        return;
                    }
                    ctx.root.store(root, Ordering::SeqCst);
                    // SAFETY: the port is live; the source belongs to it and is added to this thread's run loop.
                    let run_loop = unsafe {
                        let rl = CFRunLoopGetCurrent();
                        CFRunLoopAddSource(
                            rl,
                            IONotificationPortGetRunLoopSource(port),
                            kCFRunLoopCommonModes,
                        );
                        rl
                    };
                    let _ = tx.send(Ok(run_loop as usize));
                    while !stop_thread.load(Ordering::SeqCst) {
                        // Polls the stop flag every half second; CFRunLoopStop wakes it earlier.
                        // SAFETY: plain run loop call on this thread.
                        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.5, 0) };
                    }
                    // SAFETY: undo the registration in reverse order; each handle is used once.
                    unsafe {
                        IODeregisterForSystemPower(&raw mut notifier);
                        IOServiceClose(root);
                        IONotificationPortDestroy(port);
                    }
                    drop(ctx);
                })
                .map_err(|e| PowerError::Unavailable {
                    reason: format!("could not start the power thread: {e}"),
                })?;
            match rx.recv() {
                Ok(Ok(run_loop)) => Ok(Box::new(Watch {
                    stop,
                    run_loop,
                    thread: Some(thread),
                })),
                Ok(Err(e)) => {
                    let _ = thread.join();
                    Err(e)
                }
                Err(_) => Err(PowerError::Unavailable {
                    reason: "the power thread ended at once".into(),
                }),
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use crate::{
        EventSink, Hold, HoldRequest, PowerError, PowerEvent, PowerOs, Scope, SleepWakeSource,
        Subscription,
    };
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows_sys::Win32::System::Power::{
        DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, PowerClearRequest, PowerCreateRequest,
        PowerRegisterSuspendResumeNotification, PowerRequestDisplayRequired,
        PowerRequestSystemRequired, PowerSetRequest, PowerUnregisterSuspendResumeNotification,
    };
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };

    const DEVICE_NOTIFY_CALLBACK: u32 = 2;
    const POWER_REQUEST_CONTEXT_VERSION: u32 = 0;
    const PBT_APMSUSPEND: u32 = 4;
    const PBT_APMRESUMESUSPEND: u32 = 7;
    const PBT_APMRESUMEAUTOMATIC: u32 = 18;

    pub struct System;

    struct Request {
        handle: HANDLE,
        display: bool,
    }

    // SAFETY: a power request handle is a kernel object handle usable from any thread.
    unsafe impl Send for Request {}

    impl Hold for Request {
        fn os_ref(&self) -> String {
            format!(
                "PowerRequest {:#x}{}",
                self.handle as usize,
                if self.display { " +display" } else { "" }
            )
        }
    }

    impl Drop for Request {
        fn drop(&mut self) {
            // SAFETY: the handle came from PowerCreateRequest; clearing then closing it releases the request.
            unsafe {
                PowerClearRequest(self.handle, PowerRequestSystemRequired);
                if self.display {
                    PowerClearRequest(self.handle, PowerRequestDisplayRequired);
                }
                CloseHandle(self.handle);
            }
        }
    }

    fn last(call: &'static str) -> PowerError {
        // SAFETY: reads the calling thread's last error.
        let code = unsafe { GetLastError() };
        PowerError::Os {
            call,
            code: i64::from(code),
            detail: "Win32 error code".into(),
        }
    }

    impl PowerOs for System {
        fn acquire(&self, request: &HoldRequest) -> Result<Box<dyn Hold>, PowerError> {
            let mut reason: Vec<u16> = format!("{}: {}", request.who, request.why)
                .encode_utf16()
                .collect();
            reason.push(0);
            let context = REASON_CONTEXT {
                Version: POWER_REQUEST_CONTEXT_VERSION,
                Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
                Reason: REASON_CONTEXT_0 {
                    SimpleReasonString: reason.as_mut_ptr(),
                },
            };
            // SAFETY: `context` and the string it points to live across the call.
            let handle = unsafe { PowerCreateRequest(&raw const context) };
            if handle.is_null() || handle as isize == -1 {
                return Err(last("PowerCreateRequest"));
            }
            let display = request.scope == Scope::SystemAndDisplay;
            let held = Request {
                handle,
                display: false,
            };
            // SAFETY: the handle is live; on error `held` drops and closes it.
            if unsafe { PowerSetRequest(handle, PowerRequestSystemRequired) } == 0 {
                return Err(last("PowerSetRequest(SystemRequired)"));
            }
            let mut held = held;
            if display {
                // SAFETY: as above.
                if unsafe { PowerSetRequest(handle, PowerRequestDisplayRequired) } == 0 {
                    return Err(last("PowerSetRequest(DisplayRequired)"));
                }
                held.display = true;
            }
            Ok(Box::new(held))
        }
    }

    struct Registration {
        handle: isize,
        context: *mut EventSink,
    }

    // SAFETY: the registration handle may be unregistered from any thread.
    unsafe impl Send for Registration {}

    impl Subscription for Registration {}

    impl Drop for Registration {
        fn drop(&mut self) {
            // SAFETY: unregister first (no callback runs after it returns), then free the context once.
            unsafe {
                PowerUnregisterSuspendResumeNotification(self.handle);
                drop(Box::from_raw(self.context));
            }
        }
    }

    unsafe extern "system" fn on_power(
        context: *const c_void,
        kind: u32,
        _setting: *const c_void,
    ) -> u32 {
        // SAFETY: the context is the boxed sink kept alive until the unregistration.
        let sink = unsafe { &*context.cast::<EventSink>() };
        match kind {
            PBT_APMSUSPEND => sink(PowerEvent::Sleeping),
            PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => sink(PowerEvent::Woke),
            _ => {}
        }
        0
    }

    impl SleepWakeSource for System {
        fn subscribe(&self, sink: EventSink) -> Result<Box<dyn Subscription>, PowerError> {
            let context = Box::into_raw(Box::new(sink));
            let params = DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
                Callback: Some(on_power),
                Context: context.cast(),
            };
            let mut raw: *mut c_void = std::ptr::null_mut();
            // SAFETY: `params` lives across the call (the OS copies it); `handle` is a valid out pointer.
            let rc = unsafe {
                PowerRegisterSuspendResumeNotification(
                    DEVICE_NOTIFY_CALLBACK,
                    (&raw const params) as HANDLE,
                    &raw mut raw,
                )
            };
            if rc != 0 {
                // SAFETY: registration failed, so nothing else holds the context.
                drop(unsafe { Box::from_raw(context) });
                return Err(PowerError::Os {
                    call: "PowerRegisterSuspendResumeNotification",
                    code: i64::from(rc),
                    detail: "Win32 error code".into(),
                });
            }
            Ok(Box::new(Registration {
                handle: raw as isize,
                context,
            }))
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use crate::event_thread::{EventThread, JOIN_WAIT};
    use crate::{
        EventSink, Hold, HoldRequest, PowerError, PowerEvent, PowerOs, Scope, SleepWakeSource,
        Subscription,
    };
    use zbus::blocking::Connection;
    use zbus::zvariant::OwnedFd;

    const LOGIND: &str = "org.freedesktop.login1";
    const LOGIND_PATH: &str = "/org/freedesktop/login1";
    const LOGIND_IFACE: &str = "org.freedesktop.login1.Manager";

    pub struct System;

    fn unavailable(what: &str, e: &zbus::Error) -> PowerError {
        PowerError::Unavailable {
            reason: format!("{what}: {e}"),
        }
    }

    /// The inhibitor fd (closing it releases the lock) and, when a desktop offers it, a screen saver cookie.
    struct Inhibitor {
        _fd: OwnedFd,
        screen: Option<(Connection, u32)>,
    }

    impl Hold for Inhibitor {
        fn os_ref(&self) -> String {
            let display = match &self.screen {
                Some((_, cookie)) => format!(" ScreenSaver cookie {cookie}"),
                None => String::new(),
            };
            format!("logind inhibitor fd{display}")
        }
    }

    impl Drop for Inhibitor {
        fn drop(&mut self) {
            if let Some((conn, cookie)) = self.screen.take() {
                let _ = conn.call_method(
                    Some("org.freedesktop.ScreenSaver"),
                    "/org/freedesktop/ScreenSaver",
                    Some("org.freedesktop.ScreenSaver"),
                    "UnInhibit",
                    &(cookie,),
                );
            }
        }
    }

    fn screen_inhibit(who: &str, why: &str) -> Option<(Connection, u32)> {
        let conn = Connection::session().ok()?;
        let reply = conn
            .call_method(
                Some("org.freedesktop.ScreenSaver"),
                "/org/freedesktop/ScreenSaver",
                Some("org.freedesktop.ScreenSaver"),
                "Inhibit",
                &(who, why),
            )
            .ok()?;
        let cookie: u32 = reply.body().deserialize().ok()?;
        Some((conn, cookie))
    }

    impl PowerOs for System {
        fn acquire(&self, request: &HoldRequest) -> Result<Box<dyn Hold>, PowerError> {
            let conn = Connection::system().map_err(|e| unavailable("no system D-Bus", &e))?;
            let reply = conn
                .call_method(
                    Some(LOGIND),
                    LOGIND_PATH,
                    Some(LOGIND_IFACE),
                    "Inhibit",
                    &(
                        "sleep:idle",
                        request.who.as_str(),
                        request.why.as_str(),
                        "block",
                    ),
                )
                .map_err(|e| PowerError::Os {
                    call: "logind Inhibit",
                    code: 0,
                    detail: e.to_string(),
                })?;
            let fd: OwnedFd = reply.body().deserialize().map_err(|e| PowerError::Os {
                call: "logind Inhibit reply",
                code: 0,
                detail: e.to_string(),
            })?;
            let screen = (request.scope == Scope::SystemAndDisplay)
                .then(|| screen_inhibit(&request.who, &request.why))
                .flatten();
            Ok(Box::new(Inhibitor { _fd: fd, screen }))
        }
    }

    impl SleepWakeSource for System {
        fn subscribe(&self, sink: EventSink) -> Result<Box<dyn Subscription>, PowerError> {
            let connection =
                Connection::system().map_err(|e| unavailable("no system D-Bus", &e))?;
            let proxy = zbus::blocking::Proxy::new(&connection, LOGIND, LOGIND_PATH, LOGIND_IFACE)
                .map_err(|e| unavailable("logind", &e))?;
            let signals = proxy
                .receive_signal("PrepareForSleep")
                .map_err(|e| unavailable("logind signal", &e))?;
            // `close` ends the connection under the signal stream, which ends the thread's loop. `graceful_shutdown` would wait
            // for every clone of the connection to be dropped, and the stream (on the thread) holds one: it cannot return.
            let closing = connection.inner().clone();
            let closer = Box::new(move || {
                let _ = zbus::block_on(closing.close());
                drop(connection);
            });
            let watch = EventThread::start(
                "paddock-power-events",
                sink,
                closer,
                JOIN_WAIT,
                move |deliver| {
                    for message in signals {
                        match message.body().deserialize::<bool>() {
                            Ok(true) => deliver(PowerEvent::Sleeping),
                            Ok(false) => deliver(PowerEvent::Woke),
                            Err(_) => {}
                        }
                    }
                },
            )
            .map_err(|e| PowerError::Unavailable {
                reason: format!("could not start the power thread: {e}"),
            })?;
            Ok(Box::new(watch))
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod imp {
    use crate::{EventSink, Hold, HoldRequest, PowerError, PowerOs, SleepWakeSource, Subscription};

    pub struct System;

    impl PowerOs for System {
        fn acquire(&self, _: &HoldRequest) -> Result<Box<dyn Hold>, PowerError> {
            Err(PowerError::Unavailable {
                reason: "this OS has no keep-awake adapter".into(),
            })
        }
    }

    impl SleepWakeSource for System {
        fn subscribe(&self, _: EventSink) -> Result<Box<dyn Subscription>, PowerError> {
            Err(PowerError::Unavailable {
                reason: "this OS has no sleep notifications adapter".into(),
            })
        }
    }
}
