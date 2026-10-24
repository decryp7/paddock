//! Test doubles: a `cargo` that makes (or fails to make) the program, and a context that records the notes.

use crate::Context;
use crate::cargo::Cargo;
use crate::project::Project;
use paddock_plugin_package::manifest::host_target;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Mutex;

/// What a [`FakeCargo`] does when it is run.
enum Mode {
    /// Leaves a file where cargo would for the project.
    Make(Box<Project>),
    Nothing,
    Fail(String),
}

/// A `cargo` that runs nothing and records its arguments.
pub struct FakeCargo {
    mode: Mode,
    calls: Mutex<Vec<Vec<String>>>,
}

impl FakeCargo {
    /// Leaves the program where a real build would.
    pub fn making(project: &Project) -> FakeCargo {
        FakeCargo::new(Mode::Make(Box::new(project.clone())))
    }

    /// Succeeds and makes nothing.
    pub fn making_nothing() -> &'static FakeCargo {
        // One small value per test; leaking it gives the context the lifetime it needs.
        Box::leak(Box::new(FakeCargo::new(Mode::Nothing)))
    }

    /// Fails with `message`.
    pub fn failing(message: &str) -> FakeCargo {
        FakeCargo::new(Mode::Fail(message.into()))
    }

    fn new(mode: Mode) -> FakeCargo {
        FakeCargo {
            mode,
            calls: Mutex::new(Vec::new()),
        }
    }

    /// The arguments of every run.
    pub fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl Cargo for FakeCargo {
    fn run(&self, args: &[String], _dir: &Path) -> Result<(), String> {
        self.calls.lock().unwrap().push(args.to_vec());
        match &self.mode {
            Mode::Nothing => Ok(()),
            Mode::Fail(m) => Err(m.clone()),
            Mode::Make(p) => {
                let target = args
                    .iter()
                    .position(|a| a == "--target")
                    .map(|i| args[i + 1].clone());
                let path = p.built(target.as_deref().unwrap_or(host_target()), target.is_some());
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, b"program").unwrap();
                Ok(())
            }
        }
    }
}

/// A context in `dir` and the notes it will receive.
pub fn context<'a>(
    dir: &Path,
    cargo: &'a FakeCargo,
    quiet: bool,
) -> (Context<'a>, Rc<RefCell<Vec<String>>>) {
    let notes = Rc::new(RefCell::new(Vec::new()));
    let sink = notes.clone();
    // The note closure must outlive the context; leaking one small closure per test is the simplest correct lifetime.
    let note: &'static dyn Fn(&str) = Box::leak(Box::new(move |m: &str| {
        sink.borrow_mut().push(m.to_string())
    }));
    (
        Context {
            cwd: PathBuf::from(dir),
            cargo,
            note,
            quiet,
        },
        notes,
    )
}
