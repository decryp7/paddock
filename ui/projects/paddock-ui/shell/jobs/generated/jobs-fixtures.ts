// Generated from paddock-jobs' wire model by its tests (Rust round trip). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-jobs --features tauri --test it tauri::generated.

/** One `paddock://jobs` snapshot of every shape, as Rust serialises them: queued, discovering, determinate bytes,
 * not cancellable, done, failed, cancelled by the user and by a deadline. */
export const JOB_SNAPSHOT_FIXTURES: readonly unknown[] = [
  {
    "id": 1,
    "kind": "import",
    "title": "Importing logs",
    "subject": "3 folders",
    "state": "queued",
    "seq": 1,
    "elapsedMs": 6200,
    "etaMs": null,
    "cancellable": true,
    "notCancellableReason": null
  },
  {
    "id": 2,
    "kind": "import",
    "title": "Importing logs",
    "subject": null,
    "state": "running",
    "phase": {
      "id": "find",
      "label": "Finding files",
      "unit": "items",
      "total": null,
      "done": 12345
    },
    "seq": 3,
    "elapsedMs": 6200,
    "etaMs": null,
    "cancellable": true,
    "notCancellableReason": null
  },
  {
    "id": 3,
    "kind": "download",
    "title": "Downloading the model",
    "subject": "model.gguf",
    "state": "running",
    "phase": {
      "id": "download",
      "label": "Downloading",
      "unit": "bytes",
      "total": 123731968,
      "done": 41943040
    },
    "seq": 12,
    "elapsedMs": 5000,
    "etaMs": 9750,
    "cancellable": true,
    "notCancellableReason": null
  },
  {
    "id": 4,
    "kind": "save",
    "title": "Saving the project",
    "subject": null,
    "state": "running",
    "phase": {
      "id": "write",
      "label": "Writing",
      "unit": "items",
      "total": 40,
      "done": 3
    },
    "seq": 4,
    "elapsedMs": 0,
    "etaMs": null,
    "cancellable": false,
    "notCancellableReason": "Writing the database"
  },
  {
    "id": 5,
    "kind": "export",
    "title": "Exporting report.html",
    "subject": null,
    "state": "done",
    "seq": 3,
    "elapsedMs": 0,
    "etaMs": null,
    "cancellable": false,
    "notCancellableReason": null
  },
  {
    "id": 6,
    "kind": "scan",
    "title": "Scanning",
    "subject": null,
    "state": "failed",
    "message": "The folder is not readable",
    "seq": 3,
    "elapsedMs": 0,
    "etaMs": null,
    "cancellable": false,
    "notCancellableReason": null
  },
  {
    "id": 7,
    "kind": "scan",
    "title": "Scanning",
    "subject": null,
    "state": "cancelled",
    "reason": "user",
    "seq": 2,
    "elapsedMs": 0,
    "etaMs": null,
    "cancellable": false,
    "notCancellableReason": null
  },
  {
    "id": 8,
    "kind": "research",
    "title": "Researching",
    "subject": null,
    "state": "cancelled",
    "reason": "deadline",
    "seq": 3,
    "elapsedMs": 0,
    "etaMs": null,
    "cancellable": false,
    "notCancellableReason": null
  }
];
