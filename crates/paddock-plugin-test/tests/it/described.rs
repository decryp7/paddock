//! `check_described`: what a plugin says it contributes must be what its manifest declares.

use paddock_plugin_api::compat::check_described;
use paddock_plugin_api::{Described, Refusal};
use paddock_plugin_package::Manifest;
use paddock_plugin_test::FakeHost;
use paddock_plugin_test::fixture::{self, FixturePlugin};
use std::sync::Arc;

fn described() -> Described {
    FakeHost::start(Arc::new(FixturePlugin::new()))
        .unwrap()
        .describe()
        .unwrap()
}

fn manifest(edit: impl FnOnce(String) -> String) -> Manifest {
    Manifest::parse(&edit(fixture::manifest_toml(
        "2.0",
        "paddock-plugin-fixture",
        "",
    )))
    .unwrap()
}

fn mismatch(m: &Manifest, d: &Described) -> String {
    match check_described(m, d) {
        Err(Refusal::Mismatch { what, .. }) => what,
        other => panic!("expected a mismatch, got {other:?}"),
    }
}

#[test]
fn a_plugin_that_is_what_its_manifest_says_passes() {
    check_described(&manifest(|t| t), &described()).unwrap();
}

#[test]
fn every_difference_is_named() {
    let ok = manifest(|t| t);
    let mut d = described();
    d.id = "net.example.other".into();
    assert!(mismatch(&ok, &d).contains("calls itself"));
    let mut d = described();
    d.version = "9.9.9".into();
    assert!(mismatch(&ok, &d).contains("it is version 9.9.9"));
    let mut d = described();
    d.protocol = paddock_ipc::ProtocolVersion::new(2, 1);
    assert!(mismatch(&ok, &d).contains("it speaks protocol 2.1"));
    let mut d = described();
    d.tools[0].ops.pop();
    assert!(mismatch(&ok, &d).contains("its tools"));
    let mut d = described();
    d.views[0].writes = true;
    assert!(mismatch(&ok, &d).contains("which of them write"));
    let mut d = described();
    d.mcp_tools.clear();
    assert!(mismatch(&ok, &d).contains("MCP tools are not the manifest's"));
    let mut d = described();
    d.settings = None;
    assert!(mismatch(&ok, &d).contains("settings section"));
    let mut d = described();
    d.extensions[0].minor = 0;
    assert!(mismatch(&ok, &d).contains("the contracts it speaks"));
    // The manifest side of the same comparison: more declared than contributed.
    let without_tool = manifest(|t| {
        t.replace(
            "[[contributions]]\nkind = \"mcpTool\"\nid = \"fixture_echo\"\n",
            "",
        )
    });
    assert!(mismatch(&without_tool, &described()).contains("MCP tools"));
    // MCP tools without the permission that allows them.
    let no_permission = manifest(|t| {
        t.replace("[[permissions]]\npermission = \"mcpExpose\"\nreason = \"Offers its echo tool to assistants.\"\n", "")
    });
    assert!(
        mismatch(&no_permission, &described())
            .contains("without asking for the mcpExpose permission")
    );
}
