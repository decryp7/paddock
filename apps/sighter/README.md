<div align="center">

# Sighter

**Vision-based UI automation for apps with or without accessibility: record, label, train, and let your tests find any element.**

Sighter finds a button, a field or a piece of text on screen by sight, so legacy, custom-drawn and remote UIs can be
tested like any other. Everything runs on your machine.

</div>

![Sighter: Find any UI element by sight](docs/images/sighter-hero.svg?v=697a4097)


> **Status: in design; not usable yet.** There is no product code in this repository. The design is in
> [docs/proposal.md](docs/proposal.md) and open questions are pending. Building starts after Debrief ships.

## What it will do

![Sighter overview: record, label, train, serve, test, results, with the retrain and recovery loops](docs/images/sighter-overview.svg?v=51430f13)

![Sighter features](docs/images/sighter-features.svg?v=604d3cc2)

![How Sighter works](docs/images/sighter-how-it-works.svg?v=751b1d5c)

- **Find elements from pixels alone.** Send a screenshot (or ask for a window capture) and a query ("the Save button",
  text "Export"); get boxes, confidence and which layer answered. Accessibility trees are an optional label source,
  never required at run time.
- **Record** windows or screens plus input events, in the studio or headlessly on test machines.
- **Label and review** in a studio: suggestions from OCR, template matching, a detector and a vision model, with a
  review queue for everything not identified automatically.
- **Train** a small detector per app from your own reviewed labels, versioned and immutable (ONNX, permissive
  licences only).
- **Serve** a loopback-only API (`/v1/locate` and friends) with a layered resolver (cache, template, OCR, detector,
  vision model, "needs labelling") that your tests call from scripts, generated clients or the CLI.
- **Drive desktops, not only tests.** An MCP server and an agent mode in v1 (`sighter do "<goal>"`, Pitwall as the optional local
  planner) act on this PC or on a remote PC over RDP, including UAC prompts, under safety rules enforced in the server.
- **Stay safe.** It never clicks blindly, redacts recordings before they are stored, and sends nothing off the machine.

## Examples

> **Planned; names may change.** Nothing here is built yet. The API is one Playwright-style locator style, generated
> in every language from the same OpenAPI contract (see [the proposal](docs/proposal.md), section 7.9). Locators are lazy,
> wait for the target to be visible, stable and enabled, and are strict (two matches is an error). Every example does the
> same thing so you can compare: in a legacy Inventory app with no accessibility, find the "Part no." field, type a part
> number, click Search, read the Qty cell of the owner-drawn grid and assert "12".

<details open>
<summary><b>C#</b> (NUnit, async)</summary>

The full example: launch the exe, locate by label anchor and kind, assert with a retrying `Expect`.

```csharp
[Test] public async Task FindsPartQuantity() {
    await using var app = await Sighter.LaunchAsync(@"C:\Legacy\Inventory.exe", new SessionOptions { Model = "inventory@5" });
    var win = app.MainWindow;

    await win.GetByKind(Kind.TextBox).RightOf("Part no.").FillAsync("A-1042");   // label anchor
    await win.GetByKind(Kind.Button, name: "Search").ClickAsync();                // owner-drawn button

    var qty = win.GetByKind(Kind.Grid).Cell(row: "A-1042", column: "Qty");        // row/column header lookup
    await Expect(qty).ToHaveTextAsync("12");                                      // retries until true or timeout
}
```
</details>

<details>
<summary><b>Java</b> (JUnit 5)</summary>

The same flow with the Java client; `expect(...)` retries like the C# one.

```java
@Test void findsPartQuantity() {
    try (var app = Sighter.launch("C:\\Legacy\\Inventory.exe", SessionOptions.model("inventory@5"))) {
        var win = app.mainWindow();

        win.getByKind(Kind.TEXT_BOX).rightOf("Part no.").fill("A-1042");
        win.getByKind(Kind.BUTTON, "Search").click();

        var qty = win.getByKind(Kind.GRID).cell("A-1042", "Qty");
        expect(qty).toHaveText("12");
    }
}
```
</details>

<details>
<summary><b>Python</b> (pytest)</summary>

The same flow as a pytest test; `sighter` is a fixture that closes the app afterwards.

```python
def test_finds_part_quantity(sighter):
    app = sighter.launch(r"C:\Legacy\Inventory.exe", model="inventory@5")
    win = app.main_window

    win.get_by_kind(Kind.TEXT_BOX).right_of("Part no.").fill("A-1042")
    win.get_by_kind(Kind.BUTTON, name="Search").click()

    qty = win.get_by_kind(Kind.GRID).cell(row="A-1042", column="Qty")
    expect(qty).to_have_text("12")
```
</details>

<details>
<summary><b>TypeScript</b> (Node test runner)</summary>

The same flow with `node:test`; every action returns a promise.

```ts
import { test } from "node:test";
import { Sighter, Kind, expect } from "@sighter/client";

test("finds part quantity", async () => {
  await using app = await Sighter.launch("C:\\Legacy\\Inventory.exe", { model: "inventory@5" });
  const win = app.mainWindow;

  await win.getByKind(Kind.TextBox).rightOf("Part no.").fill("A-1042");
  await win.getByKind(Kind.Button, { name: "Search" }).click();

  const qty = win.getByKind(Kind.Grid).cell({ row: "A-1042", column: "Qty" });
  await expect(qty).toHaveText("12");
});
```
</details>

<details>
<summary><b>C++</b> (GoogleTest, synchronous client)</summary>

The same flow with the synchronous C++ client; the wait and the retry happen inside the call.

```cpp
TEST(Inventory, FindsPartQuantity) {
  auto app = sighter::Sighter::launch(R"(C:\Legacy\Inventory.exe)", {.model = "inventory@5"});
  auto win = app.main_window();

  win.get_by_kind(Kind::TextBox).right_of("Part no.").fill("A-1042");
  win.get_by_kind(Kind::Button, {.name = "Search"}).click();

  auto qty = win.get_by_kind(Kind::Grid).cell({.row = "A-1042", .column = "Qty"});
  sighter::expect(qty).to_have_text("12");
}
```
</details>

<details>
<summary><b>Hybrid with FlaUI</b> (C#)</summary>

FlaUI keeps doing what UIA can see (the window and its standard fields); Sighter covers the custom-drawn part, scoped to the element's rectangle.

```csharp
using var app = Application.Launch(@"C:\Legacy\Inventory.exe");
using var automation = new UIA3Automation();
var window = app.GetMainWindow(automation);
window.FindFirstDescendant(cf => cf.ByAutomationId("PartNo")).AsTextBox().Enter("A-1042");   // UIA is fine here

var gridPane = window.FindFirstDescendant(cf => cf.ByAutomationId("GridPane"));   // custom-drawn: UIA sees one pane
await gridPane.Sighter().GetByText("Search").ClickAsync();                         // scoped to the pane's rectangle
await Expect(gridPane.Sighter().GetByKind(Kind.Grid).Cell("A-1042", "Qty")).ToHaveTextAsync("12");
await FirstOf(window.FindFirstDescendant(cf => cf.ByAutomationId("Save")),         // UIA first, vision when it misses
              window.Sighter().GetByText("Save")).ClickAsync();
```
</details>

<details>
<summary><b>Hybrid with pywinauto</b> (Python)</summary>

The same idea in Python: pywinauto finds the dialog and the standard control, Sighter reads the owner-drawn grid inside it.

```python
from pywinauto import Application
from sighter_pywinauto import sighter           # adapter

app = Application(backend="uia").connect(title_re="Inventory.*")
dlg = app.window(title_re="Inventory.*")
dlg.child_window(auto_id="PartNo").type_keys("A-1042")        # UIA control

grid = dlg.child_window(auto_id="GridPane")                   # custom-drawn: one UIA pane
sighter(grid).get_by_text("Search").click()                   # search inside grid.rectangle()
expect(sighter(grid).get_by_kind(Kind.GRID).cell("A-1042", "Qty")).to_have_text("12")
```
</details>

<details>
<summary><b>Robot Framework</b> (<code>SighterLibrary</code>)</summary>

The same flow as keywords, for teams that write tests in Robot; the library wraps the Python client.

```robotframework
*** Settings ***
Library    SighterLibrary    model=inventory@5

*** Test Cases ***
Finds Part Quantity
    Launch App    C:\\Legacy\\Inventory.exe
    Fill          kind=textbox    right_of=Part no.    text=A-1042
    Click         kind=button     name=Search
    Cell Should Have Text    kind=grid    row=A-1042    column=Qty    expected=12
```
</details>

<details>
<summary><b>Sighter's own test</b> (TOML plan)</summary>

No code: the same check as a versionable test plan with a data table, kept in git next to the app (the format is shared with Pacenotes).

```toml
# inventory.plan.toml
format = "sighterplan"
version = 1
[plan]
id = "inventory-lookup"
title = "Inventory lookup"
app = "inventory"

[[data.parts]]                     # one run per row
part = "A-1042"
qty  = "12"
[[data.parts]]
part = "B-2210"
qty  = "3"

[[case]]
id = "TC-INV-1"
title = "Part quantity is shown"
data = "parts"
  [[case.step]]
  action = "type"
  target = { class = "textbox", right_of = "Part no." }
  value  = "{{data.part}}"
  [[case.step]]
  action = "click"
  target = { class = "button", name = "Search" }
  [[case.step]]
  action = "assert_text"
  target = { class = "grid", cell = { row = "{{data.part}}", column = "Qty" } }
  expect = "{{data.qty}}"
  timeout_ms = 5000
```

```sh
sighter run inventory.plan.toml --report junit
```
</details>

<details>
<summary><b>Multi-app</b> (C#)</summary>

One test, two applications, each with its own model: copy a part number from the Inventory app and use it in the Orders app.

```csharp
await using var inventory = await Sighter.AttachAsync(title: "Inventory");
await using var orders    = await Sighter.LaunchAsync(@"C:\Legacy\Orders.exe", new SessionOptions { Model = "orders@2" });

await inventory.ActivateAsync();
await inventory.MainWindow.GetByKind(Kind.Grid).Cell("A-1042", "Part").ClickAsync();
await Sighter.Keyboard.PressAsync("Control+C");

await orders.ActivateAsync();                                        // brings to front, waits until focused
await orders.MainWindow.GetByKind(Kind.TextBox).RightOf("Part").FillAsync(await Sighter.Clipboard.ReadAsync());
await Expect(orders.MainWindow.GetByText("Part found")).ToBeVisibleAsync();
```
</details>

<details>
<summary><b>Recovery</b> (C#, with Pitwall)</summary>

If a step gets stuck, Pitwall's vision model can diagnose why; with `DiagnoseOnly` nothing is clicked, and the failure carries the diagnosis.

```csharp
await using var app = await Sighter.LaunchAsync(@"C:\Legacy\Inventory.exe",
    new SessionOptions { Model = "inventory@5", Recovery = RecoveryPolicy.DiagnoseOnly });
try {
    await app.MainWindow.GetByKind(Kind.Button, name: "Search").ClickAsync();
} catch (ElementNotFoundException e) {
    Console.WriteLine(e.Diagnosis);       // e.g. "A 'Session expired' dialog covers the window"
    Console.WriteLine(e.ScreenshotPath);  // plus e.ReviewLink for the labelling queue
    throw;
}
```
</details>

## Desktop automation, MCP and agent mode

> **Planned; names may change.** Design only (see [the proposal](docs/proposal.md), sections 7.14 and 7.15). The `Target`
> interface, the MCP server and agent mode are v1 (agent mode decided for v1 on 2026-10-23); IP-KVM comes later.

Besides test code, Sighter can drive a desktop for people and agents. The same detection, locators and plans sit above a
small `Target` interface, so something written once runs on any target:

| Target | What it is | When |
|---|---|---|
| **Local** | Screenshots plus guarded input on this PC | v1 |
| **RDP** | A remote PC (or VM) driven from a controller PC running Sighter, through Sighter's own embedded, headless RDP client. UAC prompts and elevated apps are shown in the session, so they can be reached, which local input cannot do | v1 |
| **VNC** | A VNC client | with RDP if cheap, else later |
| **IP-KVM** | An existing open IP-KVM device, only for what RDP cannot show: BIOS/UEFI and the pre-logon screen | later, optional |

![Desktop automation components](docs/images/diagrams/svg/c4-3-desktop-automation.svg?v=0614eabe)

**MCP server.** Sighter exposes a Model Context Protocol server over stdio or loopback only. Its tools act on the
selected target:

- `observe`: a screenshot plus the detected, labelled elements with boxes (`#1 Save button`, `#2 Cancel`), so a model
  refers to elements by label and never guesses pixels;
- `observe_image`: detect and label elements on a given image (for example a recorded video frame); no input, no target;
- `locate`, `click`, `type`, `press`, `drag`, `launch_app`, `wait_for`.

Any MCP host can drive it: Sighter's own runner, [Debrief](https://dev.decryptology.net/decryp7/debrief),
[Pacenotes](https://dev.decryptology.net/decryp7/pacenotes), Claude Desktop or Claude Code, or a script. The host brings
its own model.

```json
{
  "mcpServers": {
    "sighter": { "command": "sighter", "args": ["mcp", "--target", "rdp:lab-pc-07"] }
  }
}
```

**Agent mode (v1).** `sighter do "<goal>"` runs a built-in loop with [Pitwall](https://dev.decryptology.net/decryp7/pitwall)
as the local planner. Each step is observe, the model picks one tool call, safety check, act, verify. Pitwall is optional:
without it agent mode shows a clear "needs Pitwall (or an MCP host that brings its own model)" state, while scripted runs and the
MCP server work without any LLM, and scripted plans stay the default for CI.

```sh
sighter do "Open Settings, turn on Dark mode, and confirm the title bar turns dark" --target local --max-steps 25 --timeout 5m
```

![One agent-mode step](docs/images/diagrams/svg/uml-seq-agent-step.svg?v=201c19c3)

**Targeting an RDP session.** The locator API is unchanged; only the session's target differs. The same in a plan:

```csharp
await using var pc = await Sighter.ConnectAsync(Target.Rdp("lab-pc-07", credential: "lab-admin"));   // credential by reference
await pc.Screen.GetByText("Yes").RightOf("Do you want to allow this app").ClickAsync();            // a UAC prompt
```

```toml
[plan]
id = "install-needs-uac"
target = { kind = "rdp", host = "lab-pc-07", credential = "lab-admin" }   # secret stays in the OS credential store

[[case]]
id = "TC-UAC-1"
title = "Installer asks for elevation"
  [[case.step]]
  action = "click"
  target = { class = "button", name = "Yes", right_of = "Do you want to allow this app" }
```

**Safety, enforced in Sighter's server** so it holds for every host and for scripts: allow-listed apps and windows only;
confirmation by the user (never the model) before destructive actions; a step and time budget; a visible "agent in
control" overlay with a stop hotkey, and physical input pauses the run; a full replay log kept in the run bundle and
openable in Debrief; local only, with no cloud model and no remote MCP. Credentials come from the OS credential store and
never appear in plans or logs.

### Reproduce a bug from a Debrief video

Debrief turns a screen recording into candidate steps (using `observe_image` to name the clicked elements), connects to
Sighter's MCP server, selects the RDP target of the PC under test, and drives the steps with Pitwall choosing the tools
while watching that PC's logs for the same error signature. It stops when the failure reproduces or a budget runs out,
then saves the executed steps as a `.sighterplan` regression test that later runs with no LLM. The user confirms before
anything runs on a remote PC.

![Reproduce a bug from a Debrief video](docs/images/diagrams/svg/uml-seq-repro-from-video.svg?v=16ea23e7)

## How it fits the family

Sighter is the sixth app of the Paddock family. [Pitwall](https://dev.decryptology.net/decryp7/pitwall) supplies the
local vision model, [Debrief](https://dev.decryptology.net/decryp7/debrief) opens the run bundles (screenshots, overlays,
results) for analysis, and [Pitcrew](https://dev.decryptology.net/decryp7/pitcrew) runs the tests on CI runners that have
Sighter as a service. See the
[family overview](https://dev.decryptology.net/decryp7/paddock/src/branch/main/docs/ecosystem.md).

## Documentation

- Design diagrams (all planned; sources in [docs/diagrams](docs/diagrams)):
  - [Containers (C4 level 2)](docs/images/diagrams/svg/c4-2-containers.svg?v=1b8e84ed)
  - [Locate and click an element (sequence)](docs/images/diagrams/svg/uml-seq-locate.svg?v=7b1c72d7)
  - [Recover a stuck step (sequence)](docs/images/diagrams/svg/uml-seq-recovery.svg?v=2a42aa9c)
  - [Desktop automation components](docs/images/diagrams/svg/c4-3-desktop-automation.svg?v=0614eabe)
  - [One agent-mode step (sequence)](docs/images/diagrams/svg/uml-seq-agent-step.svg?v=201c19c3)
  - [Reproduce a bug from a Debrief video (sequence)](docs/images/diagrams/svg/uml-seq-repro-from-video.svg?v=16ea23e7)
  - [Element label lifecycle (state)](docs/images/diagrams/svg/uml-state-element-label.svg?v=41a65dc5)
- [Design proposal](docs/proposal.md) (status: proposal, open questions pending)
- [Lessons from the suite](docs/suite-lessons.md)
- [Engineering rules](AGENTS.md)

## Licence

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.
