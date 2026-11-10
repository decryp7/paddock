//! `dev hooks install`.
use clap::Args;
use paddock_cli::{CliError, Report};

/// Arguments of `hooks install`.
#[derive(Args)]
pub struct InstallArgs {}

/// `dev hooks install`.
pub fn command(_a: &InstallArgs) -> Result<Report, CliError> {
    Err(CliError::failure("not yet"))
}
