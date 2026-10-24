//! The command table for the Paddock apps: one macro for the kind, window classes, request size limit and async-only rule of every command, the guard hook and generated TypeScript command names.
//!
//! ADR 0012 §3; plan S1. Taken from Debrief's `command_table!` and `commands.rs` (Debrief ADR 0016), without Debrief's
//! commands, window labels or error type.
//!
//! - [`command_table!`] declares every command once: its [`Kind`] (how it is logged), the [`Windows`] that may call it
//!   and its largest request. It builds a validated [`Table`] in a `static` (a defect is a compile error) and,
//!   optionally, a macro that expands to the Tauri invoke handler of the same list.
//! - [`check`] is the guard as a pure function: window class from the shell's label scheme ([`labels`]), the
//!   per-dialog contract through the [`DialogGrant`] hook (implemented by `paddock-dialogs`, ADR 0009), the size limit.
//!   The Tauri adapter `tauri::Guard` (feature `tauri`) runs it on every call, rejects with a [`Refusal`] and logs it.
//! - [`ts::names_module`] renders the table as TypeScript (command names as a union type); [`testing`] has the checks an
//!   app runs in its tests: every command async, generated files current.
//!
//! The core is pure Rust with no Tauri dependency; the adapter lives in `tauri` behind the `tauri` feature.
//!
//! ```
//! paddock_commands::command_table! {
//!     /// Every command of the app.
//!     pub static COMMANDS;
//!     Query [WINDOW] => workspace::summary,
//!     Mutation [MAIN | DIALOG] limit 4 * paddock_commands::MIB => workspace::open_paths,
//!     Quiet [MAIN] => session::save_session,
//! }
//! assert_eq!(COMMANDS.spec("open_paths").map(|s| s.limit()), Some(4 * 1024 * 1024));
//! assert_eq!(COMMANDS.kind_of("summary"), paddock_commands::Kind::Query);
//! ```
//!
//! A command listed twice does not compile:
//!
//! ```compile_fail,E0080
//! paddock_commands::command_table! {
//!     pub static COMMANDS;
//!     Query [WINDOW] => a::summary,
//!     Query [MAIN] => b::summary,
//! }
//! ```
//!
//! Nor does a command no window may call (`NONE`), or a limit outside 1 KiB to 64 MiB:
//!
//! ```compile_fail,E0080
//! paddock_commands::command_table! {
//!     pub static COMMANDS;
//!     Query [NONE] => a::summary,
//! }
//! ```
//!
//! ```compile_fail,E0080
//! paddock_commands::command_table! {
//!     pub static COMMANDS;
//!     Query [MAIN] limit 10 => a::summary,
//! }
//! ```

pub mod guard;
pub mod labels;
pub mod table;
pub mod testing;
pub mod ts;

pub use guard::{DialogGrant, Refusal, RefusalCode, check};
pub use labels::{Label, WindowClass, class_of};
pub use table::{
    CommandInfo, DEFAULT_LIMIT, KIB, Kind, MAX_LIMIT, MIB, MIN_LIMIT, Spec, Table, TableError,
    Windows, limit,
};

#[cfg(feature = "tauri")]
pub mod tauri;

/// Declares a command table: `Kind [CLASS | CLASS] (limit <bytes>)? => path::to::handler,` per command.
///
/// - `Kind` is a [`Kind`] variant: `Query`, `Mutation` or `Quiet`.
/// - The classes are [`Windows`] constants: `MAIN`, `POPOUT`, `DIALOG`, `WINDOW` (those three) and `TRAY` (tray
///   popovers, never part of `WINDOW`).
/// - `limit` is the largest request in bytes ([`DEFAULT_LIMIT`] when omitted); [`KIB`] and [`MIB`] help.
/// - The wire name is the handler's last path segment; doc comments on entries are allowed.
///
/// The head names the `static` [`Table`] and, optionally, a macro to define that expands to the Tauri invoke handler
/// of the same list (`::tauri::generate_handler![...]`), so the table and the handler can never disagree:
///
/// ```ignore
/// paddock_commands::command_table! {
///     pub(crate) static COMMANDS, macro handler;
///     Query [WINDOW] => workspace::summary,
/// }
/// // ...
/// .invoke_handler(paddock_commands::tauri::Guard::new(&COMMANDS).wrap(handler!()))
/// ```
///
/// The table is validated at compile time ([`Table::new`]): a duplicate name, a command callable from no window class,
/// a name that is not `lower_snake_case` or a limit outside [`MIN_LIMIT`]..=[`MAX_LIMIT`] does not compile.
///
/// A table can also be **composed** of other tables (`static NAME = [A, B, ...];`): the shell plugin folds every shell
/// crate's table into one, validated the same way, so a command two crates both declare does not compile. Each part
/// keeps its own handler; `tauri::Routes` sends a call to the handler of the part that lists it.
///
/// ```
/// paddock_commands::command_table! {
///     pub static HEALTH;
///     Query [WINDOW] => health::health_snapshot,
/// }
/// paddock_commands::command_table! {
///     pub static JOBS;
///     Mutation [WINDOW] => jobs::job_cancel,
/// }
/// paddock_commands::command_table! {
///     /// Every command of the shell.
///     pub static SHELL = [HEALTH, JOBS];
/// }
/// assert_eq!(SHELL.specs().len(), 2);
/// assert_eq!(SHELL.kind_of("job_cancel"), paddock_commands::Kind::Mutation);
/// ```
///
/// Two parts naming the same command do not compile:
///
/// ```compile_fail,E0080
/// paddock_commands::command_table! { pub static A; Query [WINDOW] => a::status, }
/// paddock_commands::command_table! { pub static B; Query [MAIN] => b::status, }
/// paddock_commands::command_table! { pub static BOTH = [A, B]; }
/// ```
#[macro_export]
macro_rules! command_table {
    (
        $(#[$table_meta:meta])* $vis:vis static $table:ident = [ $($part:path),* $(,)? ] $(;)?
    ) => {
        $(#[$table_meta])*
        $vis static $table: $crate::Table = {
            const PARTS: &[&$crate::Table] = &[ $( &$part ),* ];
            const N: usize = $crate::table::total(PARTS);
            static SPECS: [$crate::Spec; N] = $crate::table::concat::<N>(PARTS);
            $crate::Table::new(&SPECS)
        };
    };
    (
        $(#[$table_meta:meta])* $vis:vis static $table:ident, macro $handler:ident;
        $( $(#[$doc:meta])* $kind:ident [$($win:ident)|+] $(limit $limit:expr)? => $($seg:ident)::+ ),* $(,)?
    ) => {
        $crate::command_table! {
            $(#[$table_meta])* $vis static $table;
            $( $kind [$($win)|+] $(limit $limit)? => $($seg)::+ ),*
        }
        /// Expands to the Tauri invoke handler of every command of the table.
        #[allow(unused_macros)]
        macro_rules! $handler {
            () => {
                ::tauri::generate_handler![ $( $($seg)::+ ),* ]
            };
        }
    };
    (
        $(#[$table_meta:meta])* $vis:vis static $table:ident;
        $( $(#[$doc:meta])* $kind:ident [$($win:ident)|+] $(limit $limit:expr)? => $($seg:ident)::+ ),* $(,)?
    ) => {
        $(#[$table_meta])*
        $vis static $table: $crate::Table = $crate::Table::new(&[
            $( $crate::Spec::new(
                stringify!($($seg)::+),
                $crate::Kind::$kind,
                $crate::Windows::NONE $(.with($crate::Windows::$win))+,
                $crate::limit(&[$($limit)?]),
            ), )*
        ]);
    };
}

#[cfg(test)]
mod tests;
