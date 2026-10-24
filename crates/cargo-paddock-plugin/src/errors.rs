//! Turning the platform's typed errors into what the command line says: the message and the next step, and the exit code.

use paddock_cli::CliError;
use paddock_plugin_package::PackageError;

/// A package error as a command-line error. A package that was changed, is unsigned or signed by an unknown key is
/// **refused** (exit 4: do not install it); anything else is a failure of the work (exit 1).
pub fn package(e: &PackageError) -> CliError {
    let hint = match e {
        PackageError::Io(_) => "Check that the path exists and you may read and write it.",
        PackageError::NotAPackage(_) => {
            "Pass a .paddockplugin file made by `cargo paddock-plugin pack`."
        }
        PackageError::TooLarge { .. } => "Remove what the program does not need from the build.",
        PackageError::InvalidManifest(_) => "Fix manifest.toml and pack again.",
        PackageError::UnlistedFile(_)
        | PackageError::MissingFile(_)
        | PackageError::UnsafeEntry(_) => {
            "Build the package again with `cargo paddock-plugin pack`."
        }
        PackageError::Tampered(_) => {
            "Do not install this package; get it from its publisher again."
        }
        PackageError::Unsigned => {
            "Sign it with `cargo paddock-plugin sign <package> --key <file>`."
        }
        PackageError::UnknownKey { .. } => {
            "Pass the publisher's public key with --trust, or sign with a trusted key."
        }
        PackageError::Incompatible(_) => {
            "Rebuild the plugin against the current paddock-plugin-api."
        }
        PackageError::WrongTarget { .. } => "Build for this target, or pack the missing target.",
        PackageError::InvalidKey(_) => {
            "Make a key with `cargo paddock-plugin keygen --out <file>`."
        }
    };
    let error = match e {
        PackageError::Tampered(_) | PackageError::Unsigned | PackageError::UnknownKey { .. } => {
            CliError::refused(e.to_string())
        }
        _ => CliError::failure(e.to_string()),
    };
    error.with_hint(hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use paddock_cli::Exit;

    #[test]
    fn untrustworthy_packages_are_refused_and_every_error_has_a_next_step() {
        let all = [
            PackageError::Io("x".into()),
            PackageError::NotAPackage("x".into()),
            PackageError::TooLarge {
                what: "x".into(),
                limit: 1,
            },
            PackageError::InvalidManifest("x".into()),
            PackageError::UnlistedFile("x".into()),
            PackageError::MissingFile("x".into()),
            PackageError::UnsafeEntry("x".into()),
            PackageError::Tampered("x".into()),
            PackageError::Unsigned,
            PackageError::UnknownKey { key_id: "k".into() },
            PackageError::Incompatible("x".into()),
            PackageError::WrongTarget {
                target: "t".into(),
                supported: "u".into(),
            },
            PackageError::InvalidKey("x".into()),
        ];
        for e in &all {
            let c = package(e);
            assert!(!c.hint.is_empty() && c.message == e.to_string());
            let refused = matches!(e.code(), "tampered" | "unsigned" | "unknownKey");
            assert_eq!(
                c.exit,
                if refused {
                    Exit::Refused
                } else {
                    Exit::Failure
                },
                "{e}"
            );
        }
    }
}
