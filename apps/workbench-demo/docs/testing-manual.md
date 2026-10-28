# Workbench demo: manual checks

What the automated tests cannot reach (rule 7: no windows on the desktop). Run these in `npm run tauri dev` of the demo, on
each OS, before a release that touches the dock or `paddock-panels`.

## A pop-out dragged back over its window is translucent (`paddock-panels::redock`)

Set-up: pop a panel out (tab menu, Float or drag the tab out of the window), so it has an OS window of its own.

1. Drag the pop-out's title bar over the main window. Expected: the moment the pointer is over the main window the pop-out
   turns about 50 % transparent and the drop guides (compass, edge guides, preview) show through it; the guide under the pointer
   is highlighted.
2. Move the pointer back out of the main window (without releasing). Expected: the pop-out is fully opaque again.
3. Drag it over the main window and release on a guide. Expected: the panel docks there; no translucent window is left
   behind.
4. Drag it over the main window, release on no guide (title bar area). Expected: the window stays where it was dropped and is
   fully opaque.
5. Close the pop-out with the keyboard (Alt+F4 / Cmd+W) while it is over the main window, and press Escape during the drag.
   Expected: no window stays translucent and nothing is logged as an error. Escape does not cancel a title-bar move on
   every OS; where the OS ends the move loop but the button is still held, the window turns opaque when the button is
   released (the follower ends on the button, not on the key).
6. While translucent over the main window, maximise or snap it (Windows: Win+Up) or toggle "keep on top". Expected: it stays
   translucent (the opacity is applied again about every 200 ms, because the windowing layer rewrites a window's style on a state
   change).
7. With two displays, drag over the main window on the second display. Expected: as 1 to 3.

Per OS:

- **macOS.** The window uses `NSWindow.alphaValue`. Also turn on System Settings > Accessibility > Display > "Reduce
  transparency" and repeat 1: the pop-out stays opaque and the drop guides get a heavier outline instead.
- **Windows (gate: translucency is OFF by default).** A layered window (`WS_EX_LAYERED`, `SetLayeredWindowAttributes`) over a
  WebView2 child is not verified on a real desktop, so `PanelsBuilder::windows_translucency` defaults to false and the guides
  outline instead: expect an opaque pop-out and outlined guides. To flip the default to on, run 1 to 7 with
  `PADDOCK_WINDOWS_TRANSLUCENCY=1` set in the environment of the demo (dev only; it calls `.windows_translucency(true)`) on a real Windows 10 and 11 session (a physical or VM desktop, not a headless runner), at 100 %
  and 150 % scale and on a second display, and confirm: the web content is visible (not black or frozen) while translucent;
  after the drop or leaving, the window is fully opaque with no stale frame, shadow or black rectangle; maximise or snap keeps
  it translucent. Only then change the default and record the date and Windows build here. Also turn off Settings >
  Personalization > Colors > "Transparency effects" with the option on and repeat 1: opaque pop-out, outlined guides.
- **Linux, X11 (GTK).** The window uses `gtk_widget_set_opacity`, which needs a compositor. Without one, expect an opaque
  pop-out with outlined guides and one warning in the log ("cannot make a window translucent"), never an error dialog. The mouse
  button is read through GDK, so the drop ends it like on the other systems.
- **Linux, Wayland.** A client cannot read the pointer outside its own surfaces, so the cursor position is unavailable and
  the feature never starts: the pop-out stays opaque and the guides plain. Expected, not a bug.

The opacity is the app's `PanelsBuilder::redock_opacity` (default 0.5, kept between 0.25 and 0.9).
