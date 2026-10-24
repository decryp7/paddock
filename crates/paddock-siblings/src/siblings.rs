//! [`Siblings`]: what an app holds to open the other apps of the family.

use crate::app::AppSpec;
use crate::discover::{Found, discover};
use crate::error::SiblingError;
use crate::handoff::Handoff;
use crate::probe::{FileProbe, Spawner};
use paddock_env::Os;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;

/// Reads an environment variable (injected so tests never read the real one).
pub type EnvFn = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// How a hand-off ended. "Not installed" is a plain answer with the words to show, not an error (the app has not failed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum Outcome {
    /// The app was started (or, if it ran, took the arguments over).
    #[serde(rename_all = "camelCase")]
    Opened {
        /// The program that was started.
        program: String,
    },
    /// The app is not installed here.
    NotInstalled {
        /// What to tell the user.
        message: String,
    },
    /// An older version is installed that cannot take part.
    NeedsUpdate {
        /// What to tell the user.
        message: String,
    },
}

/// What is known about one sibling on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Presence {
    /// The app's id.
    pub id: String,
    /// The app's name.
    pub name: String,
    /// Its program was found.
    pub installed: bool,
    /// Where.
    pub program: Option<String>,
    /// Only the pre-rename program was found.
    pub needs_update: bool,
}

/// The other apps, as seen from `me`. Everything that touches the machine is a port.
pub struct Siblings {
    me: &'static AppSpec,
    os: Os,
    path: Option<String>,
    env: EnvFn,
    probe: Arc<dyn FileProbe>,
    spawner: Arc<dyn Spawner>,
}

impl Siblings {
    /// Siblings of the app `me` on this machine: the real `PATH`, environment, file system and process start. Unknown
    /// `me` is refused (an app outside the family is not part of hand-offs).
    #[cfg(feature = "native")]
    pub fn native(me: &str) -> Result<Siblings, SiblingError> {
        Siblings::new(
            me,
            Os::current(),
            std::env::var("PATH").ok(),
            Box::new(|k| std::env::var(k).ok()),
            Arc::new(crate::probe::DiskProbe),
            Arc::new(crate::native::ProcessSpawner),
        )
    }

    /// Siblings of `me` over the given ports (tests, or an app with its own).
    pub fn new(
        me: &str,
        os: Os,
        path: Option<String>,
        env: EnvFn,
        probe: Arc<dyn FileProbe>,
        spawner: Arc<dyn Spawner>,
    ) -> Result<Siblings, SiblingError> {
        Ok(Siblings {
            me: AppSpec::lookup(me)?,
            os,
            path,
            env,
            probe,
            spawner,
        })
    }

    fn found(&self, app: &AppSpec) -> Found {
        discover(app, self.path.as_deref(), self.os, &*self.env, &*self.probe)
    }

    /// What is known about `app` here. Unknown apps are refused.
    pub fn presence(&self, app: &str) -> Result<Presence, SiblingError> {
        let spec = AppSpec::lookup(app)?;
        let found = self.found(spec);
        Ok(Presence {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            installed: matches!(found, Found::Program(_)),
            needs_update: found == Found::NeedsUpdate,
            program: match found {
                Found::Program(p) => Some(p.display().to_string()),
                _ => None,
            },
        })
    }

    /// Every other app of the family and whether it is installed here (for a "Related apps" list).
    pub fn all(&self) -> Vec<Presence> {
        crate::app::FAMILY
            .iter()
            .filter(|a| a.id != self.me.id)
            .filter_map(|a| self.presence(a.id).ok())
            .collect()
    }

    /// Opens `app` with `handoff`. Refuses an unknown app, itself and a hand-off the app does not accept, before anything
    /// is looked for or started. A missing app is an [`Outcome`], not an error.
    pub fn open(&self, app: &str, handoff: &Handoff) -> Result<Outcome, SiblingError> {
        let spec = AppSpec::lookup(app)?;
        if spec.id == self.me.id {
            return Err(SiblingError::ToSelf(spec.name.to_string()));
        }
        let args = handoff.to_args(spec)?;
        match self.found(spec) {
            Found::Program(program) => {
                self.spawner
                    .launch(&program, &args)
                    .map_err(|why| SiblingError::LaunchFailed {
                        app: spec.name.to_string(),
                        why,
                    })?;
                log::info!(app = spec.id; "handed off to a sibling app");
                Ok(Outcome::Opened {
                    program: program.display().to_string(),
                })
            }
            Found::NeedsUpdate => Ok(Outcome::NeedsUpdate {
                message: format!(
                    "Update {0} to use this (the installed version still has the old `{1}` program).",
                    spec.name,
                    spec.legacy_cli.unwrap_or_default()
                ),
            }),
            Found::Missing => Ok(Outcome::NotInstalled {
                message: format!(
                    "{0} was not found (looked for `{1}`). Install {0}, or open the file in it by hand.",
                    spec.name, spec.cli
                ),
            }),
        }
    }

    /// An import of `files` into `app`, checked against the file system of these siblings.
    pub fn import(&self, app: &str, files: Vec<PathBuf>) -> Result<Outcome, SiblingError> {
        self.open(app, &Handoff::import(files, &*self.probe)?)
    }
}
