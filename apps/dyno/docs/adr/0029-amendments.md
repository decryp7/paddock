# 29. Amendments to ADR 0029 (saved views)

ADR 0029 itself lands with the docs branch (`docs/dyno-cached-views`); this note records where the implementation on
`feat/dyno-cached-views` differs from it, and why, so the docs landing can fold them into the record.

- **Schema.** `disk_saved` has a `packed_bytes` column (the size the chunks take in the file; the cap and eviction count it,
  while `stored_bytes` stays the uncompressed stream length the parser is bounded by). `id` is `INTEGER PRIMARY KEY AUTOINCREMENT`
  so a row id is never reused: an entry that still holds a removed id cannot be mistaken for a later row.
- **Chunk coding.** Each stored chunk starts with one tag byte (0 raw, 1 deflate). A chunk that does not shrink is stored raw.
  D3 is settled by the bench in `performance.md`: `flate2` level 1 stays on.
- **Wire.** `PROTOCOL_VERSION` goes from 3 to 4. `DiskScanStatus` gains `notSaved` (`tooBig`, `noSpace`, `failed`) beside `origin`,
  `changedBytes` and `replaces`, so a scan that was not kept says why ("Too big to keep; raise the limit in Settings").
- **Saved maps and "Read again".** A map read back from the cache cannot be retried (its hard-link bookkeeping is not saved); the
  agent answers "scan it again". Rescan is the way.
- **Software list.** The saved software list is served to the Extras page only. The changes timeline always reads a list read in
  this run, so a saved list can never look like "nothing changed".
- **Windows identity.** The volume identity is the volume GUID path plus the volume serial number, so a reformat (same GUID,
  new serial) is told from the disk that was scanned.
- **Replaced maps.** When a Rescan is Done the map it replaces is dropped from the list, not only hidden.
- **Backups.** The backup exclusion is set on macOS only; on Windows and Linux the cache folder is relied on, and the Settings
  text says so per OS.
- **Forgetting.** If the cache files cannot be deleted (Windows, an open handle), every table is dropped in place with
  `secure_delete` and the log says that, instead of claiming the files were deleted.
