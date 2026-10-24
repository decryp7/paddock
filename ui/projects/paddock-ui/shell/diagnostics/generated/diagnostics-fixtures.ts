// Generated from paddock-diag's own code by its tests (Rust round trips). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-diag --all-features --lib generated.

import type { SectionView } from '@decryp7/paddock-ui/shell/settings';

/** The "Logging" settings section at its defaults, as `settings_overview` answers it. */
export const LOGGING_SECTION_FIXTURE: SectionView = {
  "id": "pd.logging",
  "title": "Logging",
  "description": "What the app writes to its log, which a problem report (Help > Report a problem) includes. File contents are never logged.",
  "owner": "shell",
  "fields": [
    {
      "key": "level",
      "label": "Detail",
      "description": "Takes effect at once. Detailed and Everything make the log grow faster.",
      "kind": {
        "type": "enum",
        "options": [
          {
            "value": "error",
            "label": "Errors only"
          },
          {
            "value": "warn",
            "label": "Warnings and errors"
          },
          {
            "value": "info",
            "label": "Normal"
          },
          {
            "value": "debug",
            "label": "Detailed (for support cases)"
          },
          {
            "value": "trace",
            "label": "Everything"
          }
        ]
      },
      "default": "info",
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "maxFileMb",
      "label": "Size of one day's log",
      "description": "After this much in one day only errors are written until the next day.",
      "kind": {
        "type": "int",
        "min": 1,
        "max": 100,
        "step": 1,
        "unit": "MB"
      },
      "default": 20,
      "restartRequired": true,
      "advanced": false
    },
    {
      "key": "keepFiles",
      "label": "Older days kept",
      "description": "Log files older than this are removed.",
      "kind": {
        "type": "int",
        "min": 1,
        "max": 50,
        "step": 1,
        "unit": "days"
      },
      "default": 7,
      "restartRequired": true,
      "advanced": false
    }
  ],
  "values": {
    "keepFiles": 7,
    "level": "info",
    "maxFileMb": 20
  }
};
