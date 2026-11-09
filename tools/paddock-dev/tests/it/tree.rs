//! The planner's rules on this repository's real graph, and the plan read from git: the cases of `scripts/affected.test.mjs`
//! (the ones about the plan itself) moved here so they outlive the script (ADR 0022 R4). The synthetic `.gitea` trees and the
//! pure parts (base choice, matrix legs, lock diff, command line) are unit tests of `plan`, `plan_gitea` and `plan_ci`.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use paddock_dev::plan::{Options, RepoGraph, Request, affected, per_machine_apps, plan_for, read_graph};
use paddock_dev::plan_ci::{LINUX, MatrixOpts, matrix, only_app};
use paddock_dev::plan_repo::files_naming_generated;
use paddock_repo_graph::GitHistory;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

fn graph() -> &'static RepoGraph {
    static GRAPH: OnceLock<RepoGraph> = OnceLock::new();
    GRAPH.get_or_init(|| read_graph(&root(), &GitHistory::new(root(), Duration::from_secs(120))).unwrap())
}

fn per_machine() -> &'static Vec<String> {
    static APPS: OnceLock<Vec<String>> = OnceLock::new();
    APPS.get_or_init(|| per_machine_apps(&root()))
}

fn plan_of(files: &[&str], all: bool) -> Value {
    let naming = |f: &str| files_naming_generated(&root(), f);
    let opts = Options {
        all,
        per_machine_apps: per_machine().clone(),
        crates_naming: Some(&naming),
        ..Options::default()
    };
    affected(&files.iter().map(|s| s.to_string()).collect::<Vec<_>>(), graph(), &opts).unwrap()
}

fn run(files: &[&str]) -> Value {
    plan_of(files, false)
}

fn names(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap().to_string()).collect()
}

fn apps() -> Vec<String> {
    graph().apps.iter().cloned().collect()
}

fn list(v: &Value) -> Vec<&str> {
    v.as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect()
}

fn legs(plan: &Value, opts: MatrixOpts) -> Vec<Value> {
    matrix(plan, graph(), opts).unwrap()
}

fn tracked() -> Vec<String> {
    let out = paddock_proc::command("git")
        .current_dir(root())
        .args(["ls-files"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect()
}

#[test]
fn a_shared_crate_marks_crates_and_every_app_whose_cargo_graph_reaches_it_rust_half_only() {
    let r = run(&["crates/paddock-jobs/src/lib.rs"]);
    assert_eq!(r["crates"], true);
    assert_eq!(
        r["ui"], true,
        "ui cause-codes.spec.ts scans the shared crates for health cause codes"
    );
    assert_eq!(
        names(&r["apps"]),
        ["debrief", "dyno", "pitcrew", "pitwall", "shell-demo", "spotter", "workbench-demo"]
    );
    assert_eq!(r["kinds"]["shell-demo"], json!(["rust"]));
    assert_eq!(r["kinds"]["debrief"], json!(["rust"]));
    assert!(list(&r["packages"]["crates"]).contains(&"paddock-jobs"));
    assert!(list(&r["packages"]["apps/shell-demo/src-tauri"]).contains(&"paddock-shell-demo"));
}

#[test]
fn a_crate_nothing_else_uses_marks_only_crates_and_an_app_crate_only_its_app() {
    let g = graph();
    let leaf = g
        .cargo
        .iter()
        .find(|(d, n)| {
            n.workspace == "crates" && d.starts_with("crates/paddock-") && !g.cargo.iter().any(|(_, o)| o.deps.iter().any(|x| x == d))
        })
        .map(|(d, _)| d.to_string())
        .expect("a leaf crate exists");
    let r = run(&[&format!("{leaf}/src/lib.rs")]);
    assert_eq!(r["crates"], true);
    for a in list(&r["apps"]) {
        assert!(
            r["packages"].get(format!("apps/{a}/src-tauri")).is_some() || !r["packages"][format!("apps/{a}/app/src-tauri")].is_null(),
            "{a}"
        );
    }
    let r = run(&["apps/shell-demo/src-tauri/agent/src/main.rs"]);
    assert_eq!(
        (r["apps"].clone(), r["crates"].clone(), r["ui"].clone()),
        (json!(["shell-demo"]), json!(false), json!(true))
    );
    assert_eq!(
        r["packages"]["apps/shell-demo/src-tauri"],
        json!(["paddock-shell-demo", "shell-demo-agent"])
    );
}

#[test]
fn the_e2e_port_and_build_guard_mark_the_ui_half_of_every_app() {
    for f in ["ui/projects/paddock-e2e/src/env.ts", "ui/projects/paddock-e2e/src/build-guard.ts"] {
        let r = run(&[f]);
        for a in apps() {
            assert!(list(&r["apps"]).contains(&a.as_str()), "{f} reaches {a}");
        }
    }
}

#[test]
fn a_ui_change_marks_the_tauri_shells_that_embed_a_ui_build_both_halves() {
    let r = run(&["ui/projects/paddock-ui/shell/src/public-api.ts"]);
    assert_eq!(r["ui"], true);
    assert_eq!(names(&r["apps"]), ["dyno", "pitwall", "shell-demo", "spotter", "workbench-demo"]);
    assert_eq!(r["kinds"]["spotter"], json!(["ui"]));
    assert_eq!(r["kinds"]["dyno"], json!(["ui"]));
    assert_eq!(r["kinds"]["shell-demo"], json!(["rust", "ui"]));
}

#[test]
fn a_generated_ui_file_marks_the_crate_that_checks_it_or_every_crate_when_none_does() {
    let r = run(&["ui/projects/paddock-ui/shell/generated/dialogs.ts"]);
    assert!(r["crates"] == true && r["ui"] == true);
    assert!(list(&r["packages"]["crates"]).contains(&"paddock-dialogs"));
    let unknown = run(&["ui/projects/paddock-ui/shell/generated/no-such-file.json"]);
    assert_eq!(unknown["crates"], true);
    assert!(unknown["packages"]["crates"].as_array().unwrap().len() > 5);
    assert_eq!(run(&["ui/projects/paddock-ui/shell/src/public-api.ts"])["crates"], false);
}

#[test]
fn an_apps_cargo_toml_and_capabilities_run_conformance_other_app_files_do_not() {
    for f in [
        "apps/shell-demo/src-tauri/capabilities/dialog.json",
        "apps/shell-demo/src-tauri/Cargo.toml",
        "apps/debrief/app/src-tauri/capabilities/dialog.json",
    ] {
        let r = run(&[f]);
        assert_eq!(r["conformance"], true, "{f}");
        assert_eq!(r["apps"], json!([f.split('/').nth(1).unwrap()]), "{f}");
    }
    assert_eq!(run(&["apps/shell-demo/src-tauri/src/lib.rs"])["conformance"], false);
    // C100 (./dev gates check) reads every README.
    assert_eq!(run(&["apps/shell-demo/README.md"])["conformance"], true);
}

#[test]
fn a_per_machine_apps_installer_files_run_c91_and_the_shared_crates_sync_test() {
    for f in [
        "apps/dyno/app/src-tauri/tauri.conf.json",
        "apps/dyno/app/src-tauri/tauri.windows.conf.json",
        "apps/dyno/app/src-tauri/windows/per-machine.json",
        "apps/dyno/app/src-tauri/windows/installer-hooks.nsh",
        "apps/dyno/app/src-tauri/windows/paddock-per-machine.generated.nsh",
    ] {
        let r = run(&[f]);
        assert!(r["conformance"] == true && r["crates"] == true, "{f}");
        assert_eq!(r["apps"], json!(["dyno"]), "{f}");
    }
    let include = run(&["crates/paddock-setup/nsis/paddock-per-machine.nsh"]);
    assert!(include["conformance"] == true && include["crates"] == true);
    let non = run(&["apps/debrief/app/src-tauri/tauri.conf.json"]);
    assert!(
        non["conformance"] == true && non["crates"] == false,
        "a per-user app marks conformance only"
    );
    assert_eq!(run(&["apps/dyno/app/src-tauri/src/lib.rs"])["conformance"], false);
    assert_eq!(run(&["apps/dyno/app/src-tauri/icons/icon.ico"])["conformance"], false);
}

#[test]
fn uis_eslint_config_reaches_the_preset_and_docs_mark_only_the_link_check() {
    let r = run(&["eslint/src/preset.cjs"]);
    assert!(r["eslint"] == true && r["ui"] == true);
    // Built, not quoted: scripts/affected.test.mjs fails on a quoted repo markdown path no DOC_INPUTS entry declares.
    let adr = format!("docs/adr/{}", "0014-monorepo-under-paddock.md");
    let d = run(&[adr.as_str(), "crates/paddock-jobs/README.md", "README.md", "diagrams/paddock.xml"]);
    assert_eq!(
        (d["apps"].clone(), d["crates"].clone(), d["ui"].clone(), d["docs"].clone()),
        (json!([]), json!(false), json!(false), json!(true))
    );
    let c = run(&["apps/shell-demo/CHANGELOG.md"]);
    assert_eq!(
        (c["apps"].clone(), c["kinds"]["shell-demo"].clone()),
        (json!(["shell-demo"]), json!(["rust", "ui"]))
    );
    let rules = run(&["docs/rules.md"]);
    assert_eq!(
        (
            rules["conformance"].clone(),
            rules["tools"].clone(),
            rules["docs"].clone(),
            rules["apps"].clone()
        ),
        (json!(true), json!(true), json!(false), json!([]))
    );
}

#[test]
fn an_apps_rust_source_can_emit_a_cause_code_so_it_marks_ui_and_the_storage_cases_mark_dynos_rust_half() {
    let r = run(&["apps/dyno/app/src-tauri/src/health.rs"]);
    assert_eq!(r["ui"], true);
    assert!(list(&r["apps"]).contains(&"dyno") && list(&r["kinds"]["dyno"]).contains(&"rust"));
    assert_ne!(
        run(&["apps/dyno/app/src-tauri/Cargo.toml"])["ui"],
        true,
        "only the Rust sources are scanned"
    );
    for f in [
        "crates/paddock-health/src/lib.rs",
        "apps/shell-demo/src-tauri/src/main.rs",
        "apps/dyno/crates/dyno-agent/src/server.rs",
    ] {
        assert_eq!(run(&[f])["ui"], true, "{f}");
    }
    for f in [
        "crates/paddock-health/src/tests.rs",
        "crates/paddock-health/tests/it.rs",
        "apps/dyno/crates/dyno-agent/benches/b.rs",
        "tools/paddock-dev/src/plan.rs",
    ] {
        assert_ne!(run(&[f])["ui"], true, "{f}");
    }
    let s = run(&["ui/projects/paddock-ui/format/src/storage-cases.json"]);
    assert!(list(&s["apps"]).contains(&"dyno") && s["ui"] == true && list(&s["kinds"]["dyno"]).contains(&"rust"));
}

#[test]
fn global_inputs_mark_everything_an_unknown_root_file_too_and_editor_metadata_nothing() {
    for f in ["rust-toolchain.toml", "package.json", ".npmrc", "tsconfig/base.json"] {
        let r = run(&[f]);
        for k in ["all", "crates", "ui", "eslint", "conformance", "tools"] {
            assert_eq!(r[k], true, "{f} {k}");
        }
        assert_eq!(names(&r["apps"]), apps(), "{f}");
    }
    assert_eq!(run(&[".gitea/workflows/shell-demo-release.yml"])["apps"], json!(["shell-demo"]));
    assert_eq!(run(&[".gitea/workflows/shell-demo-release.yml"])["crates"], false);
    assert_eq!(run(&["Makefile"])["all"], true);
    for files in [
        vec![".gitignore", "ui/.gitignore", ".editorconfig"],
        vec![".gitea/ISSUE_TEMPLATE/bug.yaml", ".gitea/ISSUE_TEMPLATE/config.yaml"],
    ] {
        let m = run(&files);
        assert_eq!(
            (m["all"].clone(), m["apps"].clone(), m["ui"].clone(), m["docs"].clone()),
            (json!(false), json!([]), json!(false), json!(false))
        );
    }
}

#[test]
fn icons_conformance_scripts_ci_image_and_the_developer_tool_mark_what_they_feed() {
    assert_eq!(run(&["icons/shell-demo/shell-demo-macos-1024.png"])["apps"], json!(["shell-demo"]));
    assert_eq!(names(&run(&["icons/tools/build.mjs"])["apps"]), apps());
    assert_eq!(names(&run(&["conformance/src/checks/C03-icons.mjs"])["apps"]), apps());
    assert_eq!(run(&["conformance/test/core.test.mjs"])["apps"], json!([]));
    let s = run(&["scripts/publish.mjs"]);
    assert_eq!(
        (s["tools"].clone(), s["ui"].clone(), s["crates"].clone()),
        (json!(true), json!(true), json!(false))
    );
    let i = run(&["ci-image/Dockerfile"]);
    assert_eq!(
        (i["ciImage"].clone(), i["ui"].clone(), i["apps"].clone()),
        (json!(true), json!(true), json!([]))
    );
    let own = run(&["tools/paddock-dev/src/lib.rs"]);
    assert_eq!(
        (
            own["paddockDev"].clone(),
            own["tools"].clone(),
            own["crates"].clone(),
            own["apps"].clone()
        ),
        (json!(true), json!(false), json!(false), json!([]))
    );
    for f in ["dev", "dev.cmd"] {
        let r = run(&[f]);
        assert_eq!((r["paddockDev"].clone(), r["all"].clone()), (json!(true), json!(false)), "{f}");
    }
    assert_eq!(
        run(&["crates/paddock-slots/src/lib.rs"])["paddockDev"],
        true,
        "a path dependency of the tool reaches it"
    );
    assert_eq!(run(&["crates/paddock-cli/src/lib.rs"])["paddockDev"], true);
    assert_eq!(run(&[".cargo/config.toml"])["paddockDev"], true, "global inputs reach it");
    assert_eq!(run(&["tools/bootstrap.sh"])["paddockDev"], false);
}

#[test]
fn the_planner_files_mark_tools_ui_and_paddock_dev_never_an_app_and_the_rust_inputs_only_the_rust_builds() {
    for f in ["scripts/affected.test.mjs", "scripts/affected.mjs"] {
        let r = run(&[f]);
        assert_eq!(
            (
                r["all"].clone(),
                r["apps"].clone(),
                r["crates"].clone(),
                r["tools"].clone(),
                r["ui"].clone(),
                r["paddockDev"].clone()
            ),
            (json!(false), json!([]), json!(false), json!(true), json!(true), json!(true)),
            "{f}"
        );
    }
    let t = run(&["rust-toolchain.toml"]);
    assert!(t["all"] == true && t["ui"] == true && t["crates"] == true);
    let cargo_apps: Vec<String> = graph()
        .apps
        .iter()
        .filter(|a| graph().config(a).get("cargo").is_some_and(|c| !c.is_null() && c != ""))
        .cloned()
        .collect();
    for f in [".cargo/config.toml", "lint/clippy.toml"] {
        let r = run(&[f]);
        assert_eq!(
            (
                r["all"].clone(),
                r["ui"].clone(),
                r["crates"].clone(),
                r["paddockDev"].clone(),
                r["conformance"].clone()
            ),
            (json!(false), json!(false), json!(true), json!(true), json!(f.starts_with("lint/"))),
            "{f}"
        );
        assert_eq!(names(&r["apps"]), cargo_apps, "{f}");
        for a in &cargo_apps {
            assert_eq!(r["kinds"][a], json!(["rust"]), "{f} {a}");
        }
    }
}

#[test]
fn every_tracked_files_projects_are_in_the_plan_and_no_app_outside_apps_appears() {
    let known = apps();
    let files = tracked();
    for chunk in files.chunks(400) {
        for f in chunk {
            let r = run(&[f]);
            for a in list(&r["apps"]) {
                assert!(known.iter().any(|k| k == a), "{f} names unknown app {a}");
            }
        }
    }
}

#[test]
fn debrief_pacenotes_and_sighter_keep_their_own_shape() {
    let core = run(&["apps/debrief/crates/debrief-core/src/lib.rs"]);
    assert_eq!(
        (core["apps"].clone(), core["kinds"]["debrief"].clone(), core["crates"].clone()),
        (json!(["debrief"]), json!(["rust"]), json!(false))
    );
    assert!(list(&core["packages"]["apps/debrief"]).contains(&"debrief-app"));
    assert_eq!(run(&["apps/debrief/app/src/main.ts"])["kinds"]["debrief"], json!(["ui"]));
    let proc = run(&["crates/paddock-proc/src/lib.rs"]);
    assert!(
        list(&proc["apps"]).contains(&"debrief"),
        "debrief-store-worker uses paddock-proc by path"
    );
    assert_eq!(proc["kinds"]["debrief"], json!(["rust"]));
    assert_eq!(run(&["apps/debrief/docs/ai/sql-views.md"])["apps"], json!(["debrief"]));
    assert_eq!(run(&["apps/debrief/docs/adr/0001-x.md"])["apps"], json!([]));
    let all = plan_of(&[], true);
    for a in ["pacenotes", "sighter"] {
        assert!(graph().apps.contains(a), "{a} has its paddock.ci.json");
        assert!(!legs(&all, MatrixOpts::default()).iter().any(|l| l["app"] == a), "{a} is no leg");
        let d = run(&[&format!("apps/{a}/docs/x.md")]);
        assert_eq!((d["apps"].clone(), d["docs"].clone()), (json!([]), json!(true)));
    }
    let pitcrew = run(&[".gitea/workflows/pitcrew-release.yml"]);
    assert_eq!(
        (
            pitcrew["apps"].clone(),
            pitcrew["crates"].clone(),
            pitcrew["ui"].clone(),
            pitcrew["all"].clone()
        ),
        (json!(["pitcrew"]), json!(false), json!(false), json!(false))
    );
}

// ----------------------------------------------------------------------------------------------- matrix on real apps

fn leg_rows(plan: &Value, opts: MatrixOpts, app: &str) -> Vec<Value> {
    legs(plan, opts).into_iter().filter(|l| l["app"] == app).collect()
}

#[test]
fn everything_gives_every_app_both_halves_on_linux_and_the_summary_names_each_leg() {
    let plan = plan_of(&[], true);
    let all = legs(&plan, MatrixOpts::default());
    let expected: Vec<String> = graph()
        .apps
        .iter()
        .filter(|a| {
            let c = graph().config(a);
            c.get("workflow").is_none_or(|w| w.is_null()) && c.get("cargo").is_some_and(|x| !x.is_null() && x != "")
        })
        .cloned()
        .collect();
    let main_legs: Vec<(String, bool, bool)> = all
        .iter()
        .filter(|l| l["os"] == LINUX && l["e2e_shard"] == "")
        .map(|l| {
            (
                l["app"].as_str().unwrap().to_string(),
                l["rust"].as_bool().unwrap(),
                l["ui"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(main_legs, expected.iter().map(|a| (a.clone(), true, true)).collect::<Vec<_>>());
    let md = paddock_dev::plan_ci::summary(&plan, "everything", &all);
    for a in &expected {
        assert!(md.contains(&format!("| app {a} | rust + ui")), "{a}\n{md}");
    }
    for l in legs(
        &plan,
        MatrixOpts {
            narrow: true,
            webkit: false,
        },
    ) {
        assert_eq!(l["filter"], "", "everything never narrows");
    }
}

#[test]
fn a_pull_request_narrows_nextest_to_the_reached_crates() {
    let lib = leg_rows(
        &run(&["apps/shell-demo/src-tauri/src/lib.rs"]),
        MatrixOpts {
            narrow: true,
            webkit: false,
        },
        "shell-demo",
    );
    assert_eq!(lib[0]["filter"], "package(=paddock-shell-demo)");
    let agent = leg_rows(
        &run(&["apps/shell-demo/src-tauri/agent/src/main.rs"]),
        MatrixOpts {
            narrow: true,
            webkit: false,
        },
        "shell-demo",
    );
    assert_eq!(agent[0]["filter"], "");
    let leg = &legs(&run(&["apps/shell-demo/src-tauri/src/lib.rs"]), MatrixOpts::default())[0];
    assert_eq!(leg["cargo"], "apps/shell-demo/src-tauri");
    assert_eq!(leg["nextest_config"], "crates/.config/nextest-demos.toml");
    assert!(leg["prepare"].as_str().unwrap().contains("npm run shell-demo:build -w ui"));
    assert!(leg["checks"].as_str().unwrap().contains("demo-release.mjs shell-demo version"));
}

#[test]
fn pitcrew_has_a_host_leg_per_os_with_the_rust_gates_only_and_a_ui_only_change_adds_none() {
    let rust = leg_rows(
        &run(&["apps/pitcrew/crates/pitcrew-core/src/lib.rs"]),
        MatrixOpts::default(),
        "pitcrew",
    );
    let rows: Vec<(String, bool, bool)> = rust
        .iter()
        .map(|l| {
            (
                l["os"].as_str().unwrap().to_string(),
                l["rust"].as_bool().unwrap(),
                l["ui"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            ("ubuntu-latest".to_string(), true, false),
            ("windows-latest".to_string(), true, false),
            ("macos-latest".to_string(), true, false)
        ]
    );
    for host in rust.iter().filter(|l| l["os"] != LINUX) {
        for k in ["prepare", "checks", "ui_checks", "angular_dir"] {
            assert_eq!(host[k], "", "the host legs carry no commands: {k}");
        }
    }
    assert!(
        rust.iter()
            .all(|l| l["cargo"] == "apps/pitcrew" && l["process_patterns"].as_str().unwrap().contains("fake_runner"))
    );
    let ui = leg_rows(&run(&["apps/pitcrew/app/src/main.ts"]), MatrixOpts::default(), "pitcrew");
    assert_eq!(ui.len(), 1);
    assert_eq!(ui[0]["angular_dir"], "apps/pitcrew/app");
    assert!(ui[0]["ui_checks"].as_str().unwrap().contains("ng build"));
}

#[test]
fn rust_checks_windows_clippy_and_last_packages_follow_the_apps_contract() {
    let pw = leg_rows(
        &run(&["apps/pitwall/crates/pitwall-core/src/lib.rs"]),
        MatrixOpts::default(),
        "pitwall",
    );
    let linux = pw.iter().find(|l| l["os"] == LINUX).unwrap();
    assert!(
        linux["rust_checks"]
            .as_str()
            .unwrap()
            .contains("cargo llvm-cov nextest -p pitwall-core")
    );
    assert_eq!(linux["rust"], true);
    for l in pw.iter().filter(|l| l["os"] != LINUX) {
        assert_eq!(l["rust_checks"], "", "host legs run the Rust gates only");
    }
    let win = |app: &str| -> Vec<String> {
        leg_rows(
            &run(&[&format!("apps/{app}/crates/{app}-core/src/lib.rs")]),
            MatrixOpts::default(),
            app,
        )
        .iter()
        .filter(|l| l["os"] == "windows-latest")
        .map(|l| l["windows_clippy"].as_str().unwrap().to_string())
        .collect()
    };
    assert!(win("pitwall").iter().all(|v| v == "true") && !win("pitwall").is_empty());
    assert!(win("pitcrew").iter().all(|v| v == "false") && !win("pitcrew").is_empty());
    let last: std::collections::BTreeSet<&str> = pw.iter().map(|l| l["last_packages"].as_str().unwrap()).collect();
    assert_eq!(last, ["pitwall-app"].into());
    assert!(
        leg_rows(
            &run(&["apps/pitcrew/crates/pitcrew-core/src/lib.rs"]),
            MatrixOpts::default(),
            "pitcrew"
        )
        .iter()
        .all(|l| l["last_packages"] == "")
    );
}

#[test]
fn spotters_e2e_runs_in_shards_and_a_webkit_leg_without_lengthening_the_main_leg() {
    let spotter = graph().config("spotter");
    assert_eq!(spotter["e2eShards"], 2);
    assert!(list(&spotter["e2e"]).join("\n").contains("--shard=$E2E_SHARD"));
    assert!(list(&spotter["e2eWebkit"]).join("\n").contains("--project=webkit"));
    assert!(
        !list(&spotter["ui"]).iter().any(|l| l.contains("webkit")),
        "WebKit does not lengthen the main leg"
    );
}

#[test]
fn a_tags_gates_keep_one_app_both_halves_and_no_other_project() {
    let only = only_app(&plan_of(&[], true), "pitcrew");
    assert_eq!(only["apps"], json!(["pitcrew"]));
    assert_eq!(only["kinds"], json!({"pitcrew": ["rust", "ui"]}));
    for k in ["crates", "ui", "eslint", "conformance", "tools", "icons", "all"] {
        assert_eq!(only[k], false, "{k}");
    }
    let rows: Vec<(String, bool, bool, String)> = legs(
        &only,
        MatrixOpts {
            narrow: true,
            webkit: false,
        },
    )
    .iter()
    .map(|l| {
        (
            l["os"].as_str().unwrap().into(),
            l["rust"].as_bool().unwrap(),
            l["ui"].as_bool().unwrap(),
            l["filter"].as_str().unwrap().into(),
        )
    })
    .collect();
    assert_eq!(
        rows,
        [
            ("ubuntu-latest".into(), true, true, "".into()),
            ("windows-latest".into(), true, false, "".into()),
            ("macos-latest".into(), true, false, "".into())
        ]
    );
}

// ----------------------------------------------------------------------------------------- the plan read from git

fn sh(dir: &Path, args: &[&str]) -> String {
    let out = paddock_proc::command("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn ci_text(conf: &str) -> String {
    format!(
        "name: ci\non:\n  push:\njobs:\n  plan:\n    steps:\n      - uses: ./.gitea/actions/plan-only\n  ui:\n    steps:\n      - uses: ./.gitea/actions/node-caches\n  conformance: # the checks\n    steps:\n      - run: {conf}\n  crates:\n    steps:\n      - uses: ./.gitea/actions/build-folder\n"
    )
}

fn scratch() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    sh(&root, &["init", "-q", "-b", "main"]);
    std::fs::create_dir_all(root.join(".gitea/workflows")).unwrap();
    std::fs::write(root.join(".gitea/workflows/ci.yml"), ci_text("node a")).unwrap();
    sh(&root, &["add", "-A"]);
    sh(&root, &["commit", "-q", "-m", "base"]);
    (dir, root)
}

fn put(root: &Path, f: &str, text: &str) {
    let p = root.join(f);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

fn request(files: &[&str], base: Option<&str>) -> Request {
    Request {
        files: files.iter().map(|s| s.to_string()).collect(),
        all: false,
        explain: true,
        root: None,
        base: base.map(String::from),
        head: "HEAD".into(),
    }
}

fn plan_in(root: &Path, req: &Request) -> Value {
    plan_for(root, &GitHistory::new(root, Duration::from_secs(60)), req).unwrap()
}

#[test]
fn base_mode_a_committed_change_to_one_job_marks_that_job_before_is_the_base_after_the_head() {
    let (_keep, root) = scratch();
    let base = sh(&root, &["rev-parse", "HEAD"]);
    put(&root, ".gitea/workflows/ci.yml", &ci_text("node a --only C06"));
    sh(&root, &["commit", "-q", "-am", "one job"]);
    let r = plan_in(&root, &request(&[], Some(&base)));
    assert_eq!(
        (r["all"].clone(), r["conformance"].clone(), r["crates"].clone(), r["ui"].clone()),
        (json!(false), json!(true), json!(false), json!(false))
    );
}

#[test]
fn base_mode_a_committed_change_to_an_imported_helper_marks_the_job_of_the_action_that_runs_the_importing_script() {
    let (_keep, root) = scratch();
    put(
        &root,
        ".gitea/actions/build-folder/action.yml",
        "run: node \"${{ github.action_path }}/../../../conformance/ci/install-test-browser.mjs\"",
    );
    put(&root, ".gitea/actions/plan-only/action.yml", "runs: x");
    put(&root, ".gitea/actions/node-caches/action.yml", "runs: x");
    put(
        &root,
        "conformance/ci/install-test-browser.mjs",
        "import { entryFor } from './runtime-catalogue.mjs';",
    );
    put(&root, "conformance/ci/runtime-catalogue.mjs", "export const entryFor = 1;");
    sh(&root, &["add", "-A"]);
    sh(&root, &["commit", "-q", "-m", "script"]);
    let base = sh(&root, &["rev-parse", "HEAD"]);
    put(&root, "conformance/ci/runtime-catalogue.mjs", "export const entryFor = 2;");
    sh(&root, &["commit", "-q", "-am", "helper"]);
    let r = plan_in(&root, &request(&[], Some(&base)));
    assert_eq!(
        (r["all"].clone(), r["crates"].clone(), r["ui"].clone()),
        (json!(false), json!(true), json!(false))
    );
}

#[test]
fn files_mode_an_uncommitted_change_is_narrowed_a_committed_one_marks_everything() {
    let (_keep, root) = scratch();
    put(&root, ".gitea/workflows/ci.yml", &ci_text("node a --only C06"));
    let dirty = plan_in(&root, &request(&[".gitea/workflows/ci.yml"], None));
    assert_eq!(
        (dirty["all"].clone(), dirty["conformance"].clone(), dirty["crates"].clone()),
        (json!(false), json!(true), json!(false))
    );
    sh(&root, &["commit", "-q", "-am", "one job"]);
    assert_eq!(plan_in(&root, &request(&[".gitea/workflows/ci.yml"], None))["all"], true);
}

// ------------------------------------------------------------------------------------- the workflows' contracts

#[test]
fn ci_yml_wires_the_plan_job_outputs_and_the_commits_job_asks_for_its_range() {
    let ci = std::fs::read_to_string(root().join(".gitea/workflows/ci.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let wf = paddock_dev::plan_gitea::workflow_jobs(&ci).unwrap();
    let plan_job = &wf.jobs.iter().find(|(k, _)| k == "plan").unwrap().1;
    let declared = regex::Regex::new(r"(?m)^ {6}([a-z0-9_]+): \$\{\{ steps\.plan\.outputs\.([a-z0-9_]+) \}\}").unwrap();
    let written: Vec<String> = paddock_dev::plan_ci::ci_outputs(&json!({}), &[], None)
        .into_iter()
        .map(|(k, _)| k)
        .collect();
    let declared: Vec<(String, String)> = declared
        .captures_iter(plan_job)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    // A rerun of a run's failures (PLAN_SINCE_RUN) writes its own outputs besides these; a normal run leaves them empty. A
    // project boolean matters only when a job of this ci.yml reads it.
    let since_written: Vec<String> = since_outputs()
        .into_iter()
        .filter(|k| {
            let project = k.replace('_', "-");
            let is_project = paddock_dev::plan_gitea::CI_JOBS
                .iter()
                .any(|(_, ps)| ps.contains(&project.as_str()));
            !is_project
                || paddock_dev::plan_gitea::CI_JOBS
                    .iter()
                    .any(|(j, ps)| ps.contains(&project.as_str()) && wf.jobs.iter().any(|(k, _)| k == j))
        })
        .collect();
    for (name, step) in &declared {
        assert_eq!(name, step, "output {name} reads steps.plan.outputs.{step}");
        assert!(
            written.contains(step) || since_written.contains(step),
            "dev plan --ci writes {step}"
        );
    }
    let names: Vec<&String> = declared.iter().map(|(n, _)| n).collect();
    for c in regex::Regex::new(r"needs\.plan\.outputs\.([a-z0-9_]+)").unwrap().captures_iter(&ci) {
        assert!(
            names.iter().any(|n| **n == c[1]),
            "ci.yml uses needs.plan.outputs.{}, which the plan job does not declare",
            &c[1]
        );
    }
    for w in &written {
        assert!(names.contains(&w), "dev plan --ci writes {w}, which the plan job does not declare");
    }
    if plan_job.contains("PLAN_SINCE_RUN") {
        for w in &since_written {
            assert!(
                names.contains(&w),
                "dev plan --ci --since-run writes {w}, which the plan job does not declare"
            );
        }
    }
    let commits = &wf.jobs.iter().find(|(k, _)| k == "commits").expect("a commits job").1;
    assert!(
        commits.contains("needs: [plan]")
            && commits.contains("./dev plan --commit-range")
            && commits.contains("./dev plan --commit-flags")
            && commits.contains("fetch-depth: 0")
    );
    for v in ["EVENT", "REF", "BEFORE", "SHA", "BASE_REF", "PLAN_BASE", "INPUT_APP"] {
        assert!(commits.contains(&format!("{v}: ${{{{")), "{v}");
    }
    assert!(plan_job.contains("./dev plan --ci"), "the plan job calls dev plan");
}

/// Every output a since-run plan can write: one with a rerun of every job, the crate and paddock-dev legs known.
fn since_outputs() -> Vec<String> {
    use paddock_dev::plan_since::{Leg, Rerun, Scope, SincePlan};
    let reruns = paddock_dev::plan_gitea::CI_JOBS
        .iter()
        .map(|(j, _)| Rerun {
            id: 1,
            name: j.to_string(),
            key: Some(j.to_string()),
            leg: Leg {
                os: Some("ubuntu-latest".into()),
                ..Leg::default()
            },
            scope: Scope::Job("x".into()),
            vitest: vec![],
        })
        .collect();
    let plan = SincePlan {
        run: 1,
        workflow: "ci.yml".into(),
        head: String::new(),
        reruns,
        not_rerun: 0,
        everything: None,
    };
    plan.ci_outputs(&[], &mut |_| {}).into_iter().map(|(k, _)| k).collect()
}

/// A rerun of a run's failures must be able to run every job that failed and none that passed: every job of ci.yml is in the
/// planner's job table (or a rerun of it plans everything), and every job but the plan asks `rerun_jobs` for itself.
#[test]
fn every_ci_yml_job_is_in_the_job_table_and_asks_rerun_jobs() {
    let ci = std::fs::read_to_string(root().join(".gitea/workflows/ci.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let wf = paddock_dev::plan_gitea::workflow_jobs(&ci).unwrap();
    for (key, text) in &wf.jobs {
        assert!(
            paddock_dev::plan_gitea::CI_JOBS.iter().any(|(j, _)| j == key),
            "ci.yml job {key} is not in plan_gitea::CI_JOBS (and CI_JOBS of scripts/affected.mjs)"
        );
        if key == "plan" {
            continue;
        }
        let gate = format!("contains(fromJSON(needs.plan.outputs.rerun_jobs || '[\"{key}\"]'), '{key}')");
        assert!(text.contains("needs: [plan]"), "job {key} needs the plan");
        let cond = text
            .lines()
            .find(|l| l.starts_with("    if: "))
            .unwrap_or_else(|| panic!("job {key} has no if"));
        assert!(cond.contains(&gate), "job {key}: its if asks rerun_jobs ({gate})");
    }
    // The WebKit job runs in a rerun of its failure even without the webkit input or a push to main.
    let webkit = &wf.jobs.iter().find(|(k, _)| k == "e2e-webkit").unwrap().1;
    assert!(webkit.contains("|| contains(fromJSON(needs.plan.outputs.rerun_jobs || '[]'), 'e2e-webkit')"));
    // The matrices read their legs from the plan step, which writes every OS when it plans no rerun of them.
    for (job, output) in [("crate", "crate_legs"), ("paddock-dev", "paddock_dev_legs")] {
        let text = &wf.jobs.iter().find(|(k, _)| k == job).unwrap().1;
        assert!(
            text.contains(&format!("include: ${{{{ fromJSON(needs.plan.outputs.{output}) }}}}")),
            "{job}"
        );
    }
    let plan = &wf.jobs.iter().find(|(k, _)| k == "plan").unwrap().1;
    assert!(plan.contains("crate_legs=$PLAN_OS_LEGS") && plan.contains("paddock_dev_legs=$PLAN_OS_LEGS"));
}

#[test]
fn ci_yml_passes_every_field_of_a_matrix_leg_to_app_gates() {
    let ci = std::fs::read_to_string(root().join(".gitea/workflows/ci.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let gates = std::fs::read_to_string(root().join(".gitea/actions/app-gates/action.yml"))
        .unwrap()
        .replace("\r\n", "\n");
    let leg = legs(&plan_of(&[], true), MatrixOpts::default()).remove(0);
    let job = &ci[ci.find("\n  app:\n").unwrap()..];
    for key in leg.as_object().unwrap().keys().filter(|k| *k != "dir") {
        let re = regex::Regex::new(&format!(r"\$\{{\{{ matrix\.{key}( \|\| '[^']*')? \}}\}}")).unwrap();
        assert!(re.is_match(job), "ci.yml passes matrix.{key}");
    }
    for input in [
        "app",
        "cargo",
        "nextest-config",
        "prepare",
        "checks",
        "ui-checks",
        "rust",
        "ui",
        "filter",
    ] {
        assert!(gates.contains(&format!("\n  {input}:\n")), "{input}");
    }
    assert!(job.contains("include: ${{ fromJSON(needs.plan.outputs.apps) }}"));
    // A since-run leg narrowed to its failed tests adds `tests_only` (plan_since.rs), which app-gates must take.
    if gates.contains("\n  tests-only:\n") {
        assert!(
            job.contains("tests-only: ${{ matrix.tests_only }}"),
            "ci.yml passes matrix.tests_only"
        );
    }
}
