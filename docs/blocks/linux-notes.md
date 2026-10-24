## Linux notes

- Packages are a `.deb` and an AppImage built on Ubuntu 24.04; they need WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`) and
  GTK 3.
- A background service runs as a systemd **user** unit and needs no root. To keep it running when nobody is logged in,
  enable lingering once: `loginctl enable-linger "$USER"`.
- The tray icon needs an AppIndicator host (GNOME: the AppIndicator extension).
