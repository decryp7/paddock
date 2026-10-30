## macOS notes

- The app bundle and the DMG are sealed ad-hoc (`signingIdentity: "-"`) and are not notarised. The first time, open the
  app from System Settings > Privacy & Security > **Open Anyway** (macOS 15 and later no longer offers right-click > Open;
  macOS 26 asks for an administrator's name and password once more after the second **Open Anyway**).
- The hardened runtime is off while the seal is ad-hoc; it comes back with a real signing identity (rule 23).
- A background agent or login item is registered by the app itself and needs the user's approval once under System
  Settings > General > Login Items.
