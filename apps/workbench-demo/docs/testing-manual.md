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
5. Drag it over the main window and press Escape (where the OS cancels the move), or close the pop-out with the keyboard
   (Alt+F4 / Cmd+W) while over the main window. Expected: no window stays translucent; nothing is logged as an error.
6. With two displays, drag over the main window on the second display. Expected: as 1 to 3.

Per OS:

- **macOS.** The window uses `NSWindow.alphaValue`. Also turn on System Settings > Accessibility > Display > "Reduce
  transparency" and repeat 1: the pop-out stays opaque and the drop guides get a heavier outline instead.
- **Windows.** The window becomes a layered window (`WS_EX_LAYERED`, `SetLayeredWindowAttributes`) while it is over the main
  window and a plain window again after. Also turn off Settings > Personalization > Colors > "Transparency effects" and
  repeat 1: opaque pop-out, outlined guides. Check on a second display with a different scale (150 %).
- **Linux (GTK).** The window uses `gtk_widget_set_opacity`, which needs a compositor (GNOME, KDE, any Wayland session). Under a
  session without compositing, expect an opaque pop-out with outlined guides and one warning in the log ("cannot make a
  window translucent"), never an error dialog. The mouse button cannot be read on every Linux session: then the drag counts as
  over half a second after the window stops moving.

The opacity is the app's `PanelsBuilder::redock_opacity` (default 0.5, kept between 0.25 and 0.9).
