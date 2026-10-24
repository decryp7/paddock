//! Installed plugins on disk: inspect, install, update, verify, damage, and the set of plugins for the Plugins view.

use crate::common::{READ_DOCS, Rig};
use paddock_plugin_host::{Grants, PluginHost, Status};
use paddock_plugin_package::{
    PackageError, Permission, SecretKey, Trust, TrustPolicy, TrustSettings,
};
use paddock_plugin_test::fixture;
use std::path::PathBuf;

fn unsigned_package(rig: &Rig, manifest: paddock_plugin_package::Manifest) -> PathBuf {
    let program = rig.program("paddock-plugin-fixture");
    let out = rig.dir.path().join("unsigned");
    std::fs::create_dir_all(&out).unwrap();
    fixture::build_package(&out, manifest, &program, None).unwrap()
}

#[test]
fn a_signed_package_is_inspected_installed_and_read_back_with_what_the_user_granted() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", READ_DOCS),
        &program,
    );
    let inspection = rig
        .plugins
        .inspect(&pkg, &rig.trust, &rig.cfg.info)
        .unwrap();
    assert!(inspection.installable().is_ok() && inspection.installed.is_none());
    assert!(matches!(&inspection.trust, Ok(Trust::Builtin { .. })));
    assert_eq!(inspection.manifest.id.as_str(), fixture::ID);
    assert_eq!(inspection.added_permissions.len(), 2);
    assert!(inspection.removed_permissions.is_empty());

    // Granting something the manifest does not ask for is dropped at install.
    let stray = Permission::Network(vec![
        paddock_plugin_package::HostPattern::new("evil.example.com").unwrap(),
    ]);
    let asked = inspection.manifest.permissions[1].permission.clone();
    let grants = Grants::none()
        .allow(asked.clone())
        .allow(stray)
        .folder("docs", rig.dir.path());
    let installed = rig
        .plugins
        .install(&pkg, &rig.trust, &rig.cfg.info, &inspection.digest, grants)
        .unwrap();
    assert_eq!(installed.record.grants.permissions, [asked]);
    assert_eq!(
        (
            installed.record.version.as_str(),
            installed.record.trust.as_str(),
            installed.record.enabled
        ),
        ("0.1.0", "builtin", true)
    );
    assert!(installed.program.is_file() && installed.data_dir.is_dir());
    rig.plugins.verify(&installed).unwrap();
    assert_eq!(
        rig.plugins.record(fixture::ID).unwrap().unwrap(),
        installed.record
    );
    let listed = rig.plugins.list();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].as_ref().unwrap().record.id, fixture::ID);
    assert!(rig.plugins.root().ends_with("plugins"));
}

#[test]
fn the_package_must_be_the_one_the_user_reviewed_and_must_be_trusted() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
    );
    let wrong = rig
        .plugins
        .install(
            &pkg,
            &rig.trust,
            &rig.cfg.info,
            "0000000000000000",
            Grants::none(),
        )
        .unwrap_err();
    assert!(matches!(wrong, PackageError::Tampered(_)));
    assert!(rig.plugins.list().is_empty());

    // Unsigned: refused unless developer mode is on, and then installed with the warning the user accepted.
    let unsigned = unsigned_package(&rig, fixture::manifest("2.0", "paddock-plugin-fixture", ""));
    let inspection = rig
        .plugins
        .inspect(&unsigned, &rig.trust, &rig.cfg.info)
        .unwrap();
    assert_eq!(
        inspection.installable().unwrap_err(),
        PackageError::Unsigned
    );
    let err = rig
        .plugins
        .install(
            &unsigned,
            &rig.trust,
            &rig.cfg.info,
            &inspection.digest,
            Grants::none(),
        )
        .unwrap_err();
    assert_eq!(err, PackageError::Unsigned);
    let dev = TrustPolicy::new(
        &[],
        TrustSettings {
            developer_mode: true,
            keys: Vec::new(),
        },
    )
    .unwrap();
    let installed = rig
        .plugins
        .install(
            &unsigned,
            &dev,
            &rig.cfg.info,
            &inspection.digest,
            Grants::none(),
        )
        .unwrap();
    assert_eq!(installed.record.trust, "unsigned");
    assert!(installed.record.warning.unwrap().contains("not signed"));

    // Signed by a key nobody trusts.
    let other = SecretKey::generate().unwrap();
    let out = rig.dir.path().join("other");
    std::fs::create_dir_all(&out).unwrap();
    let foreign = fixture::build_package(
        &out,
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
        Some(&other),
    )
    .unwrap();
    let inspection = rig
        .plugins
        .inspect(&foreign, &rig.trust, &rig.cfg.info)
        .unwrap();
    assert!(matches!(
        inspection.trust,
        Err(PackageError::UnknownKey { .. })
    ));
}

#[test]
fn an_update_keeps_the_switch_replaces_the_old_version_and_shows_permission_changes() {
    let rig = Rig::new();
    let first = rig.install(READ_DOCS, Grants::none());
    rig.plugins.set_enabled(fixture::ID, false).unwrap();
    let old_folder = first.record.folder.clone();
    // The update asks for nothing now (version bump through the manifest text).
    let toml = fixture::manifest_toml("2.0", "paddock-plugin-fixture", "")
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    let manifest = paddock_plugin_package::Manifest::parse(&toml).unwrap();
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(manifest, &program);
    let inspection = rig
        .plugins
        .inspect(&pkg, &rig.trust, &rig.cfg.info)
        .unwrap();
    assert_eq!(inspection.installed.as_ref().unwrap().version, "0.1.0");
    assert_eq!(inspection.removed_permissions.len(), 1);
    assert!(inspection.added_permissions.is_empty());
    let second = rig
        .plugins
        .install(
            &pkg,
            &rig.trust,
            &rig.cfg.info,
            &inspection.digest,
            Grants::none(),
        )
        .unwrap();
    assert_eq!(second.record.version, "0.2.0");
    assert!(
        !second.record.enabled,
        "an update does not switch a plugin back on"
    );
    assert!(
        !rig.plugins
            .root()
            .join(fixture::ID)
            .join(&old_folder)
            .exists()
    );
    assert!(second.program.is_file());
}

#[test]
fn what_this_app_cannot_serve_cannot_be_installed_and_the_text_says_who_updates() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture");
    let too_new = rig.package(
        fixture::manifest("2.5", "paddock-plugin-fixture", ""),
        &program,
    );
    let e = rig
        .plugins
        .inspect(&too_new, &rig.trust, &rig.cfg.info)
        .unwrap()
        .installable()
        .unwrap_err();
    assert!(e.to_string().contains("needs a newer TestApp"), "{e}");
    let old = rig.package(
        fixture::manifest("1.0", "paddock-plugin-fixture", ""),
        &program,
    );
    let e = rig
        .plugins
        .inspect(&old, &rig.trust, &rig.cfg.info)
        .unwrap()
        .installable()
        .unwrap_err();
    assert!(e.to_string().contains("built for an older contract"), "{e}");
    // A package without a program for this computer.
    let toml = fixture::manifest_toml("2.0", "paddock-plugin-fixture", "").replace(
        paddock_plugin_package::manifest::host_target(),
        "riscv64gc-unknown-linux-gnu",
    );
    let mut manifest = paddock_plugin_package::Manifest::parse(&toml).unwrap();
    manifest.targets = vec!["riscv64gc-unknown-linux-gnu".into()];
    let out = rig.dir.path().join("rv");
    std::fs::create_dir_all(&out).unwrap();
    let foreign = paddock_plugin_package::archive::build(
        paddock_plugin_package::archive::Build {
            manifest,
            programs: vec![("riscv64gc-unknown-linux-gnu".into(), program.as_path())],
            sign: Some(&rig.key),
        },
        &out.join("rv.paddockplugin"),
    );
    assert!(foreign.is_ok());
    let inspection = rig
        .plugins
        .inspect(&out.join("rv.paddockplugin"), &rig.trust, &rig.cfg.info)
        .unwrap();
    assert!(matches!(
        inspection.runnable,
        Err(PackageError::WrongTarget { .. })
    ));
}

#[test]
fn verification_notices_a_changed_or_deleted_program_and_a_changed_manifest() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    rig.plugins.verify(&installed).unwrap();
    let mut bytes = std::fs::read(&installed.program).unwrap();
    bytes[0] ^= 0xff;
    std::fs::write(&installed.program, &bytes).unwrap();
    assert!(matches!(
        rig.plugins.verify(&installed),
        Err(PackageError::Tampered(_))
    ));
    std::fs::remove_file(&installed.program).unwrap();
    assert!(matches!(
        rig.plugins.verify(&installed),
        Err(PackageError::MissingFile(_))
    ));
    // A manifest that asks for more than the user saw is not the installed one.
    let manifest_file = installed
        .program
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("manifest.toml");
    let text = std::fs::read_to_string(&manifest_file).unwrap();
    std::fs::write(
        &manifest_file,
        format!("{text}\n[[permissions]]\npermission = \"openedFilePaths\"\nreason = \"sneaky\"\n"),
    )
    .unwrap();
    assert!(
        matches!(rig.plugins.load(fixture::ID), Err(PackageError::Tampered(m)) if m.contains("not the one that was installed"))
    );
    let listed = rig.plugins.list();
    assert!(matches!(&listed[0], Err((id, _)) if id == fixture::ID));
}

#[test]
fn damaged_records_and_foreign_ids_are_reported_never_a_crash() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    let record_file = rig.plugins.root().join(fixture::ID).join("install.json");
    // A record that names paths outside its folder.
    let text = std::fs::read_to_string(&record_file).unwrap();
    std::fs::write(
        &record_file,
        text.replace(&installed.record.folder, "../outside"),
    )
    .unwrap();
    assert!(
        matches!(rig.plugins.load(fixture::ID), Err(PackageError::Tampered(m)) if m.contains("outside its folder"))
    );
    // A record that is not JSON.
    std::fs::write(&record_file, "{ not json").unwrap();
    let e = rig.plugins.record(fixture::ID).unwrap_err();
    assert!(
        e.to_string().contains("not usable") && e.to_string().contains("reinstall it"),
        "{e}"
    );
    // A record that belongs to another plugin.
    let rig2 = Rig::new();
    let other = rig2.install("", Grants::none());
    let other_text =
        std::fs::read_to_string(rig2.plugins.root().join(fixture::ID).join("install.json"))
            .unwrap();
    let _ = other;
    std::fs::create_dir_all(rig.plugins.root().join("net.example.other")).unwrap();
    std::fs::write(
        rig.plugins
            .root()
            .join("net.example.other")
            .join("install.json"),
        other_text,
    )
    .unwrap();
    assert!(
        matches!(rig.plugins.record("net.example.other"), Err(PackageError::Tampered(m)) if m.contains("belongs to another plugin"))
    );
    // An id that is not an id never touches the file system.
    assert!(rig.plugins.record("../etc").is_err());
    assert!(
        rig.plugins
            .set_enabled("net.example.nothing", true)
            .is_err()
    );
    assert!(
        rig.plugins
            .set_grants("net.example.nothing", Grants::none())
            .is_err()
    );
    assert!(rig.plugins.load("net.example.nothing").unwrap().is_none());
    rig.plugins.remove("net.example.nothing", false).unwrap();
}

#[test]
fn removing_keeps_the_data_folder_on_request() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    std::fs::write(installed.data_dir.join("state"), "x").unwrap();
    rig.plugins.remove(fixture::ID, true).unwrap();
    assert!(installed.data_dir.join("state").is_file() && !installed.program.exists());
    assert!(rig.plugins.list().is_empty());
    rig.plugins.remove(fixture::ID, false).unwrap();
    assert!(!rig.plugins.root().join(fixture::ID).exists());
}

#[test]
fn the_plugins_view_lists_installs_switches_grants_and_removes() {
    let rig = Rig::new();
    let host = PluginHost::load(rig.plugins.clone(), rig.cfg.clone());
    assert!(host.list().is_empty());
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", READ_DOCS),
        &program,
    );
    let inspection = host.inspect(&pkg, &rig.trust).unwrap();
    let installed = host
        .install(&pkg, &rig.trust, &inspection.digest, Grants::none())
        .unwrap();
    let list = host.list();
    assert_eq!(list.len(), 1);
    let line = &list[0];
    assert_eq!(
        (
            line.id.as_str(),
            line.name.as_str(),
            line.version.as_str(),
            line.status.clone()
        ),
        (fixture::ID, "Fixture", "0.1.0", Status::Stopped)
    );
    assert_eq!(line.requested.len(), 2);
    assert!(
        line.contributions
            .contains(&"tool:fixture.tool".to_string())
            && line
                .contributions
                .contains(&"extension:paddock.fixture.v1".to_string())
    );
    assert!(line.tasks.is_empty());
    assert_eq!(installed.record.id, fixture::ID);

    // Start it, switch it off (persisted), grant, release, and update it in place.
    let s = host.get(fixture::ID).unwrap();
    s.invoke(fixture::TOOL, "echo", serde_json::json!({"text": "x"}))
        .unwrap();
    assert_eq!(host.list()[0].status, Status::Running);
    host.set_enabled(fixture::ID, false).unwrap();
    assert_eq!(host.list()[0].status, Status::Disabled);
    assert!(!rig.plugins.record(fixture::ID).unwrap().unwrap().enabled);
    let asked = installed.manifest.permissions[1].permission.clone();
    host.set_grants(fixture::ID, Grants::none().allow(asked.clone()))
        .unwrap();
    assert_eq!(host.list()[0].grants.permissions, [asked]);
    assert_eq!(
        rig.plugins
            .record(fixture::ID)
            .unwrap()
            .unwrap()
            .grants
            .permissions
            .len(),
        1
    );
    host.release(fixture::ID);
    host.release("net.example.nothing");
    assert!(host.set_enabled("net.example.nothing", true).is_err());
    host.install(&pkg, &rig.trust, &inspection.digest, Grants::none())
        .unwrap();
    assert_eq!(host.list()[0].status, Status::Disabled);

    // A folder that disappears is forgotten (and its process stopped); it can be installed again.
    std::fs::remove_dir_all(rig.plugins.root().join(fixture::ID)).unwrap();
    host.rescan();
    assert!(host.list().is_empty() && host.get(fixture::ID).is_none());
    host.install(&pkg, &rig.trust, &inspection.digest, Grants::none())
        .unwrap();
    // A plugin whose record is damaged is listed as refused with the reason.
    std::fs::write(
        rig.plugins.root().join(fixture::ID).join("install.json"),
        "damaged",
    )
    .unwrap();
    host.rescan();
    let damaged = host.list();
    assert_eq!(damaged.len(), 1);
    assert!(matches!(&damaged[0].status, Status::Refused(m) if m.contains("reinstall it")));
    assert!(host.get(fixture::ID).is_none());
    std::fs::remove_dir_all(rig.plugins.root().join(fixture::ID)).unwrap();
    host.rescan();
    assert!(host.list().is_empty());
}

#[test]
fn removing_stops_the_process_and_stop_all_leaves_nothing_running() {
    let rig = Rig::new();
    let host = PluginHost::load(rig.plugins.clone(), rig.cfg.clone());
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
    );
    let digest = host.inspect(&pkg, &rig.trust).unwrap().digest;
    host.install(&pkg, &rig.trust, &digest, Grants::none())
        .unwrap();
    let s = host.get(fixture::ID).unwrap();
    s.invoke(fixture::TOOL, "echo", serde_json::json!({"text": "x"}))
        .unwrap();
    host.stop_all();
    assert_eq!(s.status(), Status::Stopped);
    s.invoke(fixture::TOOL, "echo", serde_json::json!({"text": "x"}))
        .unwrap();
    host.remove(fixture::ID, false).unwrap();
    assert!(host.list().is_empty() && !rig.plugins.root().join(fixture::ID).exists());
    // A rescan of the same folder keeps the supervisors that did not change.
    host.install(&pkg, &rig.trust, &digest, Grants::none())
        .unwrap();
    let same = host.get(fixture::ID).unwrap();
    host.rescan();
    host.rescan();
    assert_eq!(host.get(fixture::ID).unwrap().id(), same.id());
}

#[test]
fn a_key_the_user_trusts_installs_as_signed_and_an_unknown_one_needs_developer_mode() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture");
    let publisher = SecretKey::generate().unwrap();
    let out = rig.dir.path().join("pub");
    std::fs::create_dir_all(&out).unwrap();
    let pkg = fixture::build_package(
        &out,
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
        Some(&publisher),
    )
    .unwrap();
    let digest = rig
        .plugins
        .inspect(&pkg, &rig.trust, &rig.cfg.info)
        .unwrap()
        .digest;
    // Signed by a key nobody trusts, with developer mode on: installed with a warning.
    let dev = TrustPolicy::new(
        &[],
        TrustSettings {
            developer_mode: true,
            keys: Vec::new(),
        },
    )
    .unwrap();
    let unknown = rig
        .plugins
        .install(&pkg, &dev, &rig.cfg.info, &digest, Grants::none())
        .unwrap();
    assert_eq!(
        (
            unknown.record.signature.as_str(),
            unknown.record.trust.as_str()
        ),
        ("unknownKey", "unsigned")
    );
    assert_eq!(unknown.record.signer, Some(publisher.public().id()));
    // The user adds the publisher's key: it installs as signed by name.
    let mut policy = TrustPolicy::new(&[], TrustSettings::default()).unwrap();
    policy
        .add_key("Acme Ltd", &publisher.public().to_text())
        .unwrap();
    let signed = rig
        .plugins
        .install(&pkg, &policy, &rig.cfg.info, &digest, Grants::none())
        .unwrap();
    assert_eq!(
        (
            signed.record.signature.as_str(),
            signed.record.trust.as_str()
        ),
        ("trusted", "signed")
    );
    assert_eq!(signed.record.signer.as_deref(), Some("Acme Ltd"));
    assert!(signed.record.warning.is_none());
}

#[test]
fn a_failed_install_leaves_nothing_behind_and_a_plugins_folder_that_cannot_be_used_is_reported() {
    let rig = Rig::new();
    let program = rig.program("paddock-plugin-fixture");
    let pkg = rig.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
    );
    let digest = rig
        .plugins
        .inspect(&pkg, &rig.trust, &rig.cfg.info)
        .unwrap()
        .digest;
    // A file where the version folder goes: moving the new version into place fails.
    let plugin_dir = rig.plugins.root().join(fixture::ID);
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::write(
        plugin_dir.join(format!("v-0.1.0-{}", &digest[..8])),
        "in the way",
    )
    .unwrap();
    let e = rig
        .plugins
        .install(&pkg, &rig.trust, &rig.cfg.info, &digest, Grants::none())
        .unwrap_err();
    assert!(matches!(e, PackageError::Io(_)), "{e}");
    let left: Vec<String> = std::fs::read_dir(&plugin_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(!left.iter().any(|n| n.starts_with(".staging-")), "{left:?}");
    assert!(rig.plugins.list().is_empty());

    // The plugins folder is a file.
    let blocked = Rig::new();
    std::fs::write(blocked.plugins.root(), "a file").unwrap();
    let program = blocked.program("paddock-plugin-fixture");
    let pkg = blocked.package(
        fixture::manifest("2.0", "paddock-plugin-fixture", ""),
        &program,
    );
    let digest = blocked
        .plugins
        .inspect(&pkg, &blocked.trust, &blocked.cfg.info)
        .unwrap()
        .digest;
    let e = blocked
        .plugins
        .install(
            &pkg,
            &blocked.trust,
            &blocked.cfg.info,
            &digest,
            Grants::none(),
        )
        .unwrap_err();
    assert!(matches!(e, PackageError::Io(_)), "{e}");
    assert!(blocked.plugins.list().is_empty());
}

#[test]
fn a_record_with_a_bad_hash_is_not_trusted_and_the_plugins_live_in_the_apps_data_folder() {
    let rig = Rig::new();
    let installed = rig.install("", Grants::none());
    let file = rig.plugins.root().join(fixture::ID).join("install.json");
    let text = std::fs::read_to_string(&file).unwrap();
    std::fs::write(&file, text.replace(&installed.record.sha256, "not-hex")).unwrap();
    assert!(
        matches!(rig.plugins.load(fixture::ID), Err(PackageError::Tampered(m)) if m.contains("no valid hash"))
    );
    rig.plugins.cleanup("net.example.nothing");
    // `of` puts them under the app's data folder.
    let app = paddock_env::AppId::new("fx", "Fixture").unwrap();
    let vars = paddock_env::MapVars::new().with("FX_HOME", rig.dir.path().join("home"));
    let dirs =
        paddock_env::AppDirs::resolve(&app, paddock_env::Os::Linux, &vars, None, true).unwrap();
    let clock: std::sync::Arc<dyn paddock_jobs::time::Clock> =
        std::sync::Arc::new(rig.clock.clone());
    let plugins = paddock_plugin_host::PluginsDir::of(&dirs, clock);
    assert_eq!(plugins.root(), rig.dir.path().join("home/data/plugins"));
}
