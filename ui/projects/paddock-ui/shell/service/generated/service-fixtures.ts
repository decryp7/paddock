// Generated from paddock-service's own code by its tests (Rust round trips). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib generated.

import type { SectionView } from '@decryp7/paddock-ui/shell/settings';

/** The "Start at login" settings section at its defaults (app "Shell Demo"), as `settings_overview` answers it. */
export const LOGIN_SECTION_FIXTURE: SectionView = {
  "id": "pd.login",
  "title": "Start at login",
  "description": "What starts when you log in to this computer.",
  "owner": "shell",
  "fields": [
    {
      "key": "launchAtLogin",
      "label": "Open at login",
      "description": "Shell Demo opens in the menu bar or tray when you log in, and its background service keeps working while the window is closed.",
      "kind": {
        "type": "bool"
      },
      "default": false,
      "restartRequired": false,
      "advanced": false
    }
  ],
  "values": {
    "launchAtLogin": false
  }
};
