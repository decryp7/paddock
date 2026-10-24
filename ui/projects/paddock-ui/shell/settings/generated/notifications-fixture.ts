// Generated from paddock-shell's own code by its tests (Rust round trips). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib notifications.

import type { SectionView } from './settings';

/** The "Notifications" settings section at its defaults, as `settings_overview` answers it. */
export const NOTIFICATIONS_SECTION_FIXTURE: SectionView = {
  "id": "pd.notifications",
  "title": "Notifications",
  "description": "When the app shows a system notification. Every message is still listed in the notification centre.",
  "owner": "shell",
  "fields": [
    {
      "key": "level",
      "label": "Show notifications for",
      "description": "A message you can already see in the window in front never notifies.",
      "kind": {
        "type": "enum",
        "options": [
          {
            "value": "off",
            "label": "Nothing"
          },
          {
            "value": "alerts",
            "label": "Problems and things that need me"
          },
          {
            "value": "all",
            "label": "Everything, including finished work"
          }
        ]
      },
      "default": "alerts",
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "quietHoursOn",
      "label": "Quiet hours",
      "description": "Hold system notifications at night.",
      "kind": {
        "type": "bool"
      },
      "default": false,
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "quietStart",
      "label": "Quiet from",
      "description": "Local time, 24-hour (HH:MM).",
      "kind": {
        "type": "text",
        "maxLen": 5,
        "pattern": "^([01]?[0-9]|2[0-3]):[0-5][0-9]$"
      },
      "default": "22:00",
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "quietEnd",
      "label": "Quiet until",
      "description": "Local time; before the start means it ends the next morning.",
      "kind": {
        "type": "text",
        "maxLen": 5,
        "pattern": "^([01]?[0-9]|2[0-3]):[0-5][0-9]$"
      },
      "default": "07:00",
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "criticalInQuietHours",
      "label": "Critical failures break the quiet",
      "description": "A failure that stops the work still notifies during quiet hours.",
      "kind": {
        "type": "bool"
      },
      "default": true,
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "cooldownSecs",
      "label": "Same problem again after",
      "description": "A problem that already notified stays quiet for this long, however often it repeats.",
      "kind": {
        "type": "int",
        "min": 0,
        "max": 3600,
        "step": 1,
        "unit": "seconds"
      },
      "default": 30,
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "burst",
      "label": "At most",
      "description": "...in the time below, whatever they are about.",
      "kind": {
        "type": "int",
        "min": 1,
        "max": 50,
        "step": 1,
        "unit": "notifications"
      },
      "default": 5,
      "restartRequired": false,
      "advanced": false
    },
    {
      "key": "burstWindowSecs",
      "label": "Within",
      "description": "",
      "kind": {
        "type": "int",
        "min": 10,
        "max": 3600,
        "step": 1,
        "unit": "seconds"
      },
      "default": 300,
      "restartRequired": false,
      "advanced": false
    }
  ],
  "values": {
    "burst": 5,
    "burstWindowSecs": 300,
    "cooldownSecs": 30,
    "criticalInQuietHours": true,
    "level": "alerts",
    "quietEnd": "07:00",
    "quietHoursOn": false,
    "quietStart": "22:00"
  }
};
