//! The Tauri adapter of `paddock-commands` (built with the `tauri` feature): [`Guard`] wraps an invoke handler (an app's
//! or a shell plugin's) with the table's checks ([`crate::check`]). A refused call is answered with the
//! [`Refusal`](crate::Refusal) as the command's error and logged: as an error naming the dialog and the command when a
//! dialog called what its contract does not declare (a bug in the app), else as a warning.

use crate::guard::{DialogGrant, check};
use crate::labels::{WindowClass, class_of};
use crate::table::{DEFAULT_LIMIT, Table};
use ::tauri::ipc::{Invoke, InvokeBody};
use ::tauri::{AppHandle, Manager, Runtime};
use std::sync::Arc;

/// Finds the grant of a dialog window: the app handle and the window's label in, the dialog's grant out.
type GrantLookup<R> = dyn Fn(&AppHandle<R>, &str) -> Option<Box<dyn DialogGrant>> + Send + Sync;

/// The table's checks around an invoke handler.
pub struct Guard<R: Runtime> {
    table: &'static Table,
    grants: Option<Arc<GrantLookup<R>>>,
}

impl<R: Runtime> Guard<R> {
    /// A guard for `table`. Without [`Guard::dialog_grants`], dialog windows may call nothing of the table.
    pub fn new(table: &'static Table) -> Self {
        Guard {
            table,
            grants: None,
        }
    }

    /// The hook of the per-dialog contracts (ADR 0009): `lookup(app, label)` gives the grant of the dialog window
    /// `label`, or `None` when the dialog layer does not know it (then it may call nothing). Asked only for dialog windows.
    pub fn dialog_grants<G: DialogGrant + 'static>(
        mut self,
        lookup: impl Fn(&AppHandle<R>, &str) -> Option<G> + Send + Sync + 'static,
    ) -> Self {
        self.grants = Some(Arc::new(move |app: &AppHandle<R>, label: &str| {
            lookup(app, label).map(|g| Box::new(g) as Box<dyn DialogGrant>)
        }));
        self
    }

    /// `inner` with the checks in front: a refused call is rejected and logged and never reaches `inner`.
    pub fn wrap(
        self,
        inner: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    ) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
        let Guard { table, grants } = self;
        move |invoke: Invoke<R>| {
            let command = invoke.message.command().to_string();
            let label = invoke.message.webview_ref().label().to_string();
            let cap = table.spec(&command).map_or(DEFAULT_LIMIT, |s| s.limit());
            let size = body_size(invoke.message.payload(), cap.saturating_add(1));
            let grant = match (&grants, class_of(&label)) {
                (Some(lookup), Some(WindowClass::Dialog)) => {
                    lookup(invoke.message.webview_ref().app_handle(), &label)
                }
                _ => None,
            };
            if let Err(refusal) = check(table, &command, &label, grant.as_deref(), size) {
                let level = if refusal.breaches_contract() {
                    log::Level::Error
                } else {
                    log::Level::Warn
                };
                log::log!(
                    target: "paddock_commands",
                    level,
                    Command = command.as_str(),
                    Window = label.as_str(),
                    Dialog = refusal.dialog.as_deref().unwrap_or(""),
                    ErrorCode = format!("{:?}", refusal.code).as_str();
                    "{}", refusal.log_message()
                );
                invoke.resolver.reject(refusal);
                return true;
            }
            inner(invoke)
        }
    }
}

/// One invoke handler over several tables: a call goes to the handler of the first table that lists its command; a
/// command no table lists is not handled (Tauri answers that it does not exist). The shell plugin composes the shell
/// crates' handlers with it behind one [`Guard`] over the composed table.
pub struct Routes<R: Runtime> {
    routes: Vec<(&'static Table, Box<Handler<R>>)>,
}

/// An invoke handler, boxed.
type Handler<R> = dyn Fn(Invoke<R>) -> bool + Send + Sync;

impl<R: Runtime> Default for Routes<R> {
    fn default() -> Self {
        Routes { routes: Vec::new() }
    }
}

impl<R: Runtime> Routes<R> {
    /// No routes yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The commands of `table` go to `handler`.
    pub fn route(
        mut self,
        table: &'static Table,
        handler: impl Fn(Invoke<R>) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.routes.push((table, Box::new(handler)));
        self
    }

    /// The table that serves `command`, if any.
    pub fn table_of(&self, command: &str) -> Option<&'static Table> {
        self.routes
            .iter()
            .find(|(t, _)| t.spec(command).is_some())
            .map(|(t, _)| *t)
    }

    /// The one invoke handler.
    pub fn handler(self) -> impl Fn(Invoke<R>) -> bool + Send + Sync + 'static {
        let Routes { routes } = self;
        move |invoke: Invoke<R>| {
            let command = invoke.message.command();
            match routes.iter().find(|(t, _)| t.spec(command).is_some()) {
                Some((_, handler)) => handler(invoke),
                None => false,
            }
        }
    }
}

/// The size of a request body, counting up to about `cap` (a JSON body is measured as serialised, so a huge request
/// costs little to refuse).
pub fn body_size(body: &InvokeBody, cap: usize) -> usize {
    match body {
        InvokeBody::Raw(bytes) => bytes.len(),
        InvokeBody::Json(v) => json_size(v, cap),
    }
}

/// About the serialised size of `v`, stopping once past `cap`.
pub fn json_size(v: &serde_json::Value, cap: usize) -> usize {
    use serde_json::Value;
    match v {
        Value::Null | Value::Bool(_) => 5,
        Value::Number(_) => 20,
        Value::String(s) => s.len() + 2,
        Value::Array(items) => {
            let mut n = 2;
            for i in items {
                n += json_size(i, cap) + 1;
                if n > cap {
                    break;
                }
            }
            n
        }
        Value::Object(map) => {
            let mut n = 2;
            for (k, i) in map {
                n += k.len() + 4 + json_size(i, cap);
                if n > cap {
                    break;
                }
            }
            n
        }
    }
}

#[cfg(test)]
mod tests;
