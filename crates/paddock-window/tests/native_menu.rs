//! The native tray's menu and icon builder, built on the main thread (macOS builds menus there only, and the test
//! harness runs tests on worker threads, so this file has no harness). Nothing is shown: the builder is made, never
//! built.

use paddock_window::tauri::NativeTray;
use paddock_window::tray::{TrayEntry, TrayView, disabled, item, submenu};

fn main() {
    // cargo-nextest first asks every test binary for its tests (`--list --format terse`, then the same with `--ignored`)
    // and then runs each by name; answer the listing like libtest so this harness-less test runs under nextest too.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--list") {
        if !args.iter().any(|a| a == "--ignored") {
            println!("native_menu: test");
        }
        return;
    }
    let app = tauri::test::mock_app();
    let view = TrayView::new(
        "App",
        vec![
            item("pd.show", "Show"),
            TrayEntry::Separator,
            submenu(
                "Recent",
                vec![item("r:0", "a"), TrayEntry::Separator, disabled("none")],
            ),
        ],
        "idle",
    );
    let icon = tauri::image::Image::new_owned(vec![0; 4], 1, 1);
    let tray = NativeTray::new(move |name| (name == "idle").then(|| icon.clone()));
    assert!(tray.icon_builder(app.handle(), &view).is_ok());
    let none = NativeTray::new(|_| None);
    let missing = none.icon_builder(app.handle(), &TrayView::new("App", vec![], "busy"));
    assert!(missing.is_err(), "no image and no window icon");
    println!("native_menu: ok");
}
