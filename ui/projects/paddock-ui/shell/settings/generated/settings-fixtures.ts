// Generated from paddock-settings' own code by its tests (Rust round trips). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-settings --all-features --lib generated.

import type { LoadProblem, SetResult, SettingsChanged, SettingsOverview } from './settings';

/** The shell demo's sections (Appearance, General, Storage: every field kind) at their defaults, as `settings_overview`
 * answers. */
export const SETTINGS_OVERVIEW_FIXTURE: SettingsOverview = {
  "problem": null,
  "restartPending": false,
  "sections": [
    {
      "description": "How every window of the app looks. Changes apply at once.",
      "fields": [
        {
          "advanced": false,
          "default": "system",
          "description": "Light or dark, or follow the operating system.",
          "key": "theme",
          "kind": {
            "options": [
              {
                "label": "Same as the system",
                "value": "system"
              },
              {
                "label": "Light",
                "value": "light"
              },
              {
                "label": "Dark",
                "value": "dark"
              }
            ],
            "type": "enum"
          },
          "label": "Theme",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": "system",
          "description": "Stronger borders and no translucency, or follow the operating system.",
          "key": "contrast",
          "kind": {
            "options": [
              {
                "label": "Same as the system",
                "value": "system"
              },
              {
                "label": "Standard",
                "value": "standard"
              },
              {
                "label": "High contrast",
                "value": "more"
              }
            ],
            "type": "enum"
          },
          "label": "Contrast",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": "system",
          "description": "Reduced: fades only, no movement. Or follow the operating system.",
          "key": "motion",
          "kind": {
            "options": [
              {
                "label": "Same as the system",
                "value": "system"
              },
              {
                "label": "Reduced",
                "value": "reduce"
              },
              {
                "label": "Full",
                "value": "full"
              }
            ],
            "type": "enum"
          },
          "label": "Motion",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": "standard",
          "description": "How much space controls and rows take.",
          "key": "density",
          "kind": {
            "options": [
              {
                "label": "Comfortable",
                "value": "comfortable"
              },
              {
                "label": "Standard",
                "value": "standard"
              },
              {
                "label": "Compact",
                "value": "compact"
              }
            ],
            "type": "enum"
          },
          "label": "Density",
          "restartRequired": false
        },
        {
          "advanced": true,
          "default": "",
          "description": "An app's colour pack over the theme; empty for none. Never applied in high contrast.",
          "key": "pack",
          "kind": {
            "maxLen": 128,
            "pattern": "(?:[a-z][a-z0-9]*(?:\\.[a-z0-9]+(?:-[a-z0-9]+)*)+)?",
            "type": "text"
          },
          "label": "Theme pack",
          "restartRequired": false
        }
      ],
      "id": "pd.appearance",
      "owner": "shell",
      "title": "Appearance",
      "values": {
        "contrast": "system",
        "density": "standard",
        "motion": "system",
        "pack": "",
        "theme": "system"
      }
    },
    {
      "description": "How the demo greets you.",
      "fields": [
        {
          "advanced": false,
          "default": "Hello",
          "description": "Letters, spaces and commas.",
          "key": "greeting",
          "kind": {
            "maxLen": 40,
            "pattern": "[A-Za-z ,]+",
            "type": "text"
          },
          "label": "Greeting",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": "casual",
          "description": "",
          "key": "style",
          "kind": {
            "options": [
              {
                "label": "Casual",
                "value": "casual"
              },
              {
                "label": "Formal",
                "value": "formal"
              }
            ],
            "type": "enum"
          },
          "label": "Style",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": true,
          "description": "Announce each greeting.",
          "key": "notify",
          "kind": {
            "type": "bool"
          },
          "label": "Show a notification",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": 1,
          "description": "How often one command greets.",
          "key": "repeat",
          "kind": {
            "max": 10,
            "min": 1,
            "step": 1,
            "type": "int",
            "unit": "times"
          },
          "label": "Repeat",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": 0.5,
          "description": "",
          "key": "volume",
          "kind": {
            "max": 1.0,
            "min": 0.0,
            "step": 0.1,
            "type": "float",
            "unit": null
          },
          "label": "Volume",
          "restartRequired": true
        }
      ],
      "id": "sd.general",
      "owner": "app",
      "title": "General",
      "values": {
        "greeting": "Hello",
        "notify": true,
        "repeat": 1,
        "style": "casual",
        "volume": 0.5
      }
    },
    {
      "description": "Where the demo keeps its data.",
      "fields": [
        {
          "advanced": false,
          "default": "",
          "description": "Empty: the app's folder.",
          "key": "folder",
          "kind": {
            "dir": true,
            "type": "path"
          },
          "label": "Data folder",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": 3600000,
          "description": "",
          "key": "keep",
          "kind": {
            "maxMs": 86400000,
            "minMs": 60000,
            "type": "duration"
          },
          "label": "Keep logs for",
          "restartRequired": false
        },
        {
          "advanced": false,
          "default": 67108864,
          "description": "",
          "key": "cache",
          "kind": {
            "maxBytes": 1073741824,
            "minBytes": 1048576,
            "type": "size"
          },
          "label": "Cache size",
          "restartRequired": false
        },
        {
          "advanced": true,
          "default": [
            "*.tmp"
          ],
          "description": "One per line.",
          "key": "ignore",
          "kind": {
            "maxItems": 20,
            "type": "stringList"
          },
          "label": "Ignored names",
          "restartRequired": false
        }
      ],
      "id": "sd.storage",
      "owner": "app",
      "title": "Storage",
      "values": {
        "cache": 67108864,
        "folder": "",
        "ignore": [
          "*.tmp"
        ],
        "keep": 3600000
      }
    }
  ]
};

/** `settings_set` refusing a greeting that does not match its pattern and a repeat below its minimum. */
export const SETTINGS_REFUSED_FIXTURE: SetResult = {
  "applied": false,
  "errors": [
    {
      "key": "greeting",
      "message": "Does not have the expected format ([A-Za-z ,]+)."
    },
    {
      "key": "repeat",
      "message": "Must be between 1 times and 10 times."
    }
  ],
  "restartRequired": false,
  "values": {
    "greeting": "Hello",
    "notify": true,
    "repeat": 1,
    "style": "casual",
    "volume": 0.5
  }
};

/** The event after the volume (a restart-only field) changed. */
export const SETTINGS_CHANGED_FIXTURE: SettingsChanged = {
  "restartPending": true,
  "section": "sd.general",
  "values": {
    "greeting": "Hello",
    "notify": true,
    "repeat": 1,
    "style": "casual",
    "volume": 0.8
  }
};

/** A damaged settings file at start-up. */
export const SETTINGS_PROBLEM_FIXTURE: LoadProblem = {
  "keptAs": "settings.json.corrupt",
  "kind": "damaged",
  "message": "The settings file could not be read (not JSON: EOF while parsing a value at line 1 column 28). Defaults are in use; the damaged file was kept as settings.json.corrupt."
};
