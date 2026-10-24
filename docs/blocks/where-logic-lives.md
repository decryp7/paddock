## Where logic lives

- **The Rust core is the single source of truth** for data and behaviour: parsing, rules, state machines, configuration,
  analysis and exports. Results are computed in Rust and returned through ports and Tauri commands.
- **Angular is the presentation layer**: layout, interaction state (selection, zoom, open panels, unsaved form edits),
  rendering and formatting of what the engine returns. It must not re-implement domain rules. If the UI needs a
  computation (a preview, a validation, a count), add an engine operation and a command for it instead of writing it in
  TypeScript. Small purely visual transforms are fine.
- **The mock backend** exists for browser development and UI tests only. It mimics the contract with plausible data; it
  is never the reference for behaviour, and a feature is not "done" until it works against the real engine.
- Validation that matters is done in Rust; the UI may validate early for convenience, but the engine re-validates.
