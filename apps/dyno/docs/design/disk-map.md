# Disk map (Dyno 1.0.0)

- Status: design proposal, 2026-10-28, revised after the independent review of `098daa12a` (ER-23); no code yet
- Scope approved for 1.0: sunburst with click-to-drill, breadcrumb, hover details, a live progressive scan with phases
  and Cancel, a hidden-space segment, big and old files, duplicate hints, a virtualized table view with keyboard
  navigation, a choice of volume (internal, external, network) or folder
- Read-only ([ADR 0021](../adr/0021-monitoring-only.md), branch `dyno/monitoring-only-docs`): the disk map **never
  deletes, moves or renames anything**. Its only actions are *Reveal in Finder* (Explorer, the file manager on Linux) and
  *Copy path*. There is no "Move to Trash", no "Clean", no drag-out.
- Shared parts: `pd-sunburst` (new, spec in [docs/plans/pd-sunburst.md](../../../../docs/plans/pd-sunburst.md)),
  `pd-table` with tree rows (an addition, same spec), `pd-job`, `SuEmptyState`, `SuErrorState`, `PdPathText`,
  `PdCopyMenu`, `formatBytes`.
- Related: family ADR 0008 (jobs), [0018](../../../../docs/adr/0018-bounded-loading-everywhere.md) (bounded answers,
  virtualization by default), [0019](../../../../docs/adr/0019-limits-explain-themselves.md) (limits explain
  themselves), [0024](../../../../docs/adr/0024-background-work-contract.md) (long work is a job, background priority);
  the [design guide](../../../../docs/design-guide.md); the UI review patterns (one primary, hover row actions, tables
  not cards, charts must mean something).
- Mockups: [design/disk-map/](disk-map/) (SVG source and PNG at 2x, light and dark, 1440 x 900). The data in them is
  invented (user `alex`, `nas.local`), never a real machine's.

## 1. What the user gets

"Where did my disk go?" answered in one look, then one click at a time: a ring chart of the chosen volume or folder
where each ring is one folder level; clicking a segment makes that folder the centre; a breadcrumb and the centre
itself lead back. Beside it, the same level as a list with sizes, and three more lists for the same folder: largest
files, oldest large files, duplicate hints. The space the folders don't explain (purgeable space, snapshots, other
volumes in the container) is one labelled segment, not a mystery.

## 2. Layout (1440 x 900, Standard density)

Page frame as every Dyno page (UI rule 16): 24 px gutter, title 18/600, one muted subtitle, a 44 px toolbar.

| Region | Content |
|---|---|
| Header | Title "Disk map"; subtitle `Macintosh HD · APFS · internal · scanned 2 min ago` (network: `nas.local/Media · SMB network volume`); one primary on the right: **Rescan** (done), **Retry** (failed), none while scanning (Cancel lives in the job strip) |
| Toolbar (44) | Volume picker button (drive glyph + name + chevron) · divider · Back (icon button, disabled at the root) · breadcrumb (`Macintosh HD › Users › alex › Library › Developer`, the last crumb 600, earlier ones links) · right: segmented **Chart / Table** |
| Capacity strip | `616.5 GB used of 994.7 GB`, `378.2 GB free`; one 8 px bar: counted (neutral bar), not counted yet (ghost hatch, only while scanning or after a stop), hidden space (hatch), free (track); a legend with values. No accent: nothing here is a hero series or a threshold |
| Main, Chart view | Left card 616 px: the sunburst (outer radius 280, centre 92, four rings 58/50/40/34 px) and a two-item key ("Hidden space", "Other folders and small items"). The **Duplicate hints** tab shows a count only after the hint phase of a whole scan has run, and the count is the number of groups under the centre. Right card (flex): tabs **In this folder (n) · Largest files · Oldest files · Duplicate hints (n)**, a 28 px sticky header (Name · Size · Share), 32 px rows; below the rows the breakdown of hidden space (volume root) or the centre's details (path with Copy, items, newest change, duplicate hints, **Reveal in Finder**, **Copy path**); a muted one-line footer hint |
| Main, Table view | One full-width card: filter field, `pd-table` in tree mode (Name with swatch · Size · bar · % of centre · Items · Newest change · sticky "…" column), footer with the keyboard hint and the row count |
| Job strip (scanning) | `pd-job` between the toolbar and the cards: title, phase label, growing count `1,116,387 items · 325.9 GB of 568.6 GB counted`, determinate bar (volume) or indeterminate bar with growing count (folder), ETA only when `etaMs` is present, **Cancel** |

Below 1100 px the right card moves under the chart (the chart keeps a 440 px minimum); below 760 px the Chart view
falls back to the Table view with a one-line note, because a ring under 200 px cannot carry four levels.

## 3. Interactions

| Input | Chart | Table |
|---|---|---|
| Click / Enter | A folder segment becomes the centre (drill in). The centre circle drills **out** one level ("Up to Library") | Enter makes the row the centre (the chart follows when shown); Right opens the row in place (tree), Left closes it or moves to the parent |
| Back / up | Back button, a breadcrumb crumb, the centre circle; keys: `⌘[` / `Alt+Left` (back), `⌘↑` / `Alt+Up` (enclosing folder, as in Finder and Explorer). No Backspace binding: it would clash with the filter field | same |
| Root | `⌘⇧↑` / `Ctrl+Home` | same |
| Hover / focus | The segment and its ancestors stay at full opacity, the rest fade to 32 %; a 2 px text-colour outline marks it; the centre shows **name, size, % of parent, items**; a caption at the card's top-left shows its path; the matching row in the list is highlighted | row hover: up to 2 icon buttons (Reveal, Copy path) and "…", as everywhere (UI rule 2) |
| Context menu | Right-click, `Shift+F10`, the Menu key: **Open** (drill), **Reveal in Finder**, **Copy path**, **Copy size** (no destructive item exists) | same menu, same keys |
| Hidden space | Not drillable; click selects it and the list shows its breakdown with a sentence per part ("macOS frees it when space is needed") | a row with the lock glyph, no chevron |
| "Other" | A folded segment ("37 smaller items") is drillable into a list only: it opens the Table view filtered to those items, because a sunburst of slivers explains nothing | a normal row |

**Keyboard in the chart** follows the WAI-ARIA tree pattern so a screen reader announces what it expects: the chart is
one tab stop (`role="tree"`, roving tabindex over `role="treeitem"` segments with `aria-level`, `aria-setsize`,
`aria-posinset`, `aria-label="Developer, 92.6 GB, 55% of Library, 297,155 items"`). Up/Down = previous/next
**sibling only** (same parent, clockwise; it stops at the ends, never jumps to a cousin), Right = first child (one ring
outwards), Left = parent (one ring inwards): levels change only with Left/Right. Enter = drill
in, Home/End = first/last sibling, type-ahead by name. The focus ring is the family's 2 px accent ring drawn as an
arc outline 3 px outside the segment. **The table is the declared accessible equivalent** (section 6); the SVG tree
semantics are experimental: WebKit's support for ARIA tree roles on SVG elements is unverified in WKWebView with
VoiceOver (Dyno's macOS shell), so the tree is a convenience on top, tested per platform, and nothing depends on it.

**Zoom animation (drill in and out).** Each segment's (start angle, end angle, inner, outer radius) is interpolated
from the old layout to the new one: the clicked segment opens to 360° and moves to ring 1 while its ancestors shrink
into the centre; out is the reverse. 240 ms, `cubic-bezier(0.3, 0, 0.2, 1)` (move), driven by one `requestAnimationFrame`
loop over precomputed tweens (no CSS transform per path), labels fade in after the move (100 ms). With
`prefers-reduced-motion: reduce` or `data-motion="reduce"` the new layout replaces the old in one frame with a ≤ 100 ms
opacity cross-fade (design guide §9). Live scan updates never animate (§9: "live charts do not animate between ticks").

## 4. Colour, labels and "never colour alone"

- **Colour by branch, re-assigned at every centre.** The five largest children of the centre take chart slots 1-5 in
  size order; every other child and every folded "Other" is the neutral slot 8 (grey); hidden space is a hatch of
  `--pd-text-subtle` on `--pd-surface-2`; "not counted yet" is a fainter hatch with a dashed `--pd-border-strong`
  outline. Descendants keep their branch hue. Re-assigning on drill is deliberate (otherwise a drilled view is one hue);
  the zoom animation carries the eye across the change. Colour by file kind is **not** in 1.0 (it needs per-folder kind
  breakdowns from the engine; YAGNI until asked).
- **Siblings inside a branch alternate two tones** (base, and 12 % towards `--pd-text`) so neighbours of one hue stay
  apart; ring and radial gaps of 1.8 px in the card colour separate every segment.
- **Measured** (`ring.ts` method: Machado 2009 severity 1, CIE76 ΔE, the token palette; script in the review folder):

  | Check | Dark | Light | Target |
  |---|---|---|---|
  | Ring-adjacent branch pairs (slots 1-5, grey, wrap-around), any tone, all four visions | ΔE 21.7 | ΔE 23.5 | ≥ 20 (guide §4) |
  | Same, base tones only | 25.7 | 25.5 | ≥ 20 |
  | Segment fill vs card (`--pd-surface-1`), worst slot and tone | 6.58:1 | 3.54:1 (slot 2 orange) | ≥ 3:1 (SC 1.4.11) |
  | Sibling tone step (normal vision) | ΔE 3.0 | ΔE 4.5 | texture only, not a carrier of meaning |
  | Label ink on a fill (ink chosen per fill, label omitted below) | ≥ 4.5:1 | ≥ 4.5:1 | 4.5:1 |

  A depth tint (outer rings lighter or darker) was tried and dropped: four visible depth steps pulled the outer-ring
  branch pairs below ΔE 20 (19.9 dark at 18 %), and in light mode a tint towards white drops orange under 3:1.
- **Never colour alone.** Every coloured segment has its name in three other places: the curved direct label (ring 1
  and 2 segments wide enough for the text; ring 1 with its size), the centre on hover or focus, and the list or table
  row with the same swatch. Hidden space and "Other" also differ by pattern and by their key entry.

## 5. Small segments ("Other") and bounds

All of these are inputs of `pd-sunburst` with these Dyno values, and the engine bounds are Settings under
**Performance › Limits** (ADR 0018 rule 3, ADR 0019).

| Bound | Value | Why |
|---|---|---|
| Rings shown | 4 below the centre | DaisyDisk-like legibility at 280 px; deeper levels are one click away |
| Minimum segment angle | 1.5° (about 7 px at the outer ring, 5 px of colour after the 1.8 px gaps) | 0.6° gave 1 px slivers, a barcode (review crop `crop-done-light-slivers.png`); smaller children fold |
| Children per parent | 48 | more cannot be labelled or told apart |
| Folding | children under the angle or past 48 fold into one grey "n smaller items" segment per parent, sized as their sum, drillable into the table | sizes still add up to the parent |
| Rendered segments | at most 1,000 (hard 2,000); when exceeded the minimum angle rises until it fits, and the key says "Some small items are grouped" | keeps one `<path>` per segment cheap (SVG, ≤ 1,000 nodes) |
| Live updates | coalesced to at most 4 per second (`requestAnimationFrame` + 250 ms), layout recomputed only for changed subtrees (keyed by node id) | a 2,000-node layout must stay under 4 ms on the Mac mini (budget test) |
| Lists | `pd-table` virtual from 100 rows (ADR 0018), engine pages of 256 | |

**Never going backwards, and how the bar reaches 100 %.** For a local volume the ring's scale is fixed before the
walk: used bytes from the OS. Before and during the walk, hidden space shows **only the parts measured up front**
(purgeable space, the other volumes of the container, the page or hibernation file; section 9.6). Everything else
is the ghost "Not counted yet" = used − measured hidden − counted, which only shrinks while counted segments only
grow. The remainder ("Snapshots and files Dyno can't read" = used − counted − measured) is known only when the walk
ends: at that moment the ghost, whatever is left of it, becomes that hatched remainder in one step, so counted +
measured hidden + remainder = used exactly, and the bar is full. If counted exceeds used − measured (hard links into
another volume, APFS clones counted per copy), the remainder is 0 and the strip says "Counted more than used: clones
are counted once per copy" instead of a negative value. A cancelled or failed scan keeps the ghost as "Not counted yet".

For a folder, and for a **network share** (section 8), no total is known: the scale is the bytes counted so far, so
arcs keep their order but their angles can shift; the job bar is indeterminate with a growing count, the chart redraws
at most twice a second without tweening, and every number (size, items) only grows.

## 6. Accessible alternative: the table view

The Chart / Table switch is a view of the same state (centre, selection, sort); switching keeps both. The table is
`pd-table` in tree mode (`role="treegrid"`, `aria-level`, `aria-expanded`, `aria-rowcount`/`aria-rowindex` across the
virtual window): Name (swatch matching the chart, glyph, name, middle-ellipsis with tooltip), Size, a neutral share bar,
% of the centre, Items, Newest change (newest modification inside), a sticky "…" column. Sorted by size by default, sortable by every column, filter
field. Arrow keys move, Right opens a row in place, Enter makes it the centre, `⌘↑` / `Alt+Up` goes up (Backspace
stays with the filter field), Shift+F10 opens the menu. Every fact the chart shows is in a column; nothing exists only
in the chart. This view is the declared accessible equivalent of the chart (WCAG 1.1.1 and 2.1.1 are met here).

## 7. States (mockups in [disk-map/](disk-map/))

| State | Mockup | What it shows |
|---|---|---|
| Loading a stored result | `disk-map-skeleton-*` | Skeleton capacity strip, a skeleton ring at final size and 9 skeleton rows (`SuSkeleton`, static in reduced motion); no layout shift |
| Not scanned (empty) | `disk-map-empty-*` | Capacity strip from the OS (measured hidden parts only, the rest "Not counted yet"); `SuEmptyState`: 32 px glyph, "See what fills Macintosh HD", one sentence, one primary **Scan Macintosh HD** (a folder is chosen in the picker) |
| Nothing readable (macOS first run without Full Disk Access) | `disk-map-fda-*` | `SuErrorState` with the lock glyph in danger: "Dyno can't read Macintosh HD", why in one sentence, three numbered steps (System Settings › Privacy & Security › Full Disk Access, turn on Dyno, come back and Retry), **Retry** (primary) and **Open Full Disk Access settings** (opens the pane only, through `x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles`; Dyno changes nothing there and cannot). Shown when the engine reports `FullDiskAccess::Denied` (section 9.13) or when every top folder is unreadable. On Windows and Linux the same state says which folders and why (access denied, needs administrator) |
| Scanning | `disk-map-scanning-*` | `pd-job` with phase, count, determinate bar, ETA, Cancel; segments appear as folders finish; the ghost shrinks; hidden space shows the measured parts, the remainder "After the scan"; no duplicate-hint count yet |
| Cancelled | `disk-map-cancelled-*` | Info callout "Partial result: stopped by you at 56 %"; the partial index stays; unfinished folders marked "incomplete" in the list; the ghost stays "Not counted yet"; duplicate hints "need a whole scan"; **Rescan** is the primary |
| Done | `disk-map-done-*` | Volume root; hidden space with all three parts and the bar at 100 % |
| Hover | `disk-map-hover-*` | Path isolation, outline, centre details, path caption, row highlight |
| Drilled in | `disk-map-drilled-*` | `Developer` as centre, breadcrumb, "Up to Library" in the centre, keyboard focus on `Xcode`, details with Reveal in Finder and Copy path; the hints badge (2) equals the groups under Developer |
| Table | `disk-map-table-*` | Tree table with two rows open, the focused row, Newest change |
| Partly unreadable | `disk-map-unreadable-*` | A folder scan of `/Users/alex` without Full Disk Access: warning callout "6 folders could not be read" with **Show the folders**; the parent marked "incomplete"; a "Not readable" list with the reason per folder. Never silent (ADR 0019 rule 6) |
| Volume picker | `disk-map-picker-*` | CDK menu grouped Internal / External / Network; free space per volume ("of the server" for a share); a share that did not answer its `statfs` within the limit shows the warning glyph and "Not answering"; "Choose a folder…" (`⌘O`) |
| Error: network share dropped | `disk-map-error-network-*` | Danger callout "Scan stopped: nas.local stopped answering", what is kept, what Retry does; header primary **Retry**; the share drawn against what was counted (no ghost in the ring); the strip shows the server volume with "Not counted yet or other data on the server" |
| Stale | done | Subtitle "scanned 3 days ago"; the result is never refreshed behind the user's back |
| Folder is empty | | `SuEmptyState` inside the chart card: "This folder is empty" + **Go up** |

## 8. Volumes: internal, external, network, or any folder

- **Picker** (CDK menu, in-app): groups **Internal**, **External** (USB, Thunderbolt, SD; Time Machine disks labelled),
  **Network** (SMB, NFS, AFP, WebDAV, sshfs when mounted), then **Choose a folder…** (native folder dialog, `⌘O`).
  Each row: glyph, name, file system and bus (`APFS · USB`), free space and a neutral usage bar; "Free space not
  reported" when the volume can't say. Only mounted volumes are listed; the list updates on mount and unmount events
  (no polling, ADR 0024 rule 2). System-hidden volumes (macOS `/System/Volumes/*`, Linux pseudo file systems, snap
  loops) are not listed; they appear in hidden space where they share a container.
- **Network volumes are slower and can drop.** The subtitle says "SMB network share"; the job shows a rate
  (`1,850 items/s`) so slowness reads as progress, not a hang; the Network group says "Slower to scan; a dropped link
  stops the scan". Walk parallelism is bounded per host (ER-20): **2 concurrent directory reads per server**, 4 for an
  SSD, 1 for a spinning disk (rotational flag per OS), all at `Background` class (ADR 0024 rule 1). Network volumes are
  never scanned automatically, only on a click.
- **A share is not its server's volume.** `statfs` on an SMB or NFS mount reports the whole server volume, not the
  share. So a share is scanned like a folder: indeterminate progress, the ring scaled to what was counted, no ghost in
  the ring. The capacity strip says "Server volume: 5.70 TB used of 7.80 TB" and labels the rest "Other data on the
  server" (during a scan: "Not counted yet or other data on the server"). Only when the engine can tell the mount is
  the whole export (NFS export root with `f_fsid` equal to the server volume's, or an SMB administrative root share) is
  it treated as a volume with a fixed scale.
- **Blocked calls are abandoned, not waited for.** On a hard NFS mount or a dead SMB session, `stat`, `readdir` and
  `statfs` can block in the kernel uninterruptibly; no timeout or signal frees the thread. So network walkers run on
  **abandonable worker threads** (ADR 0020 mode-3 leases with a waiver of kind `lease`): Cancel marks the job cancelled
  and returns at once, the coordinator stops waiting, and any worker still inside a syscall is detached, its result
  discarded when (if ever) the call returns. At most 4 abandoned workers per host; while a host has abandoned workers,
  a new scan of it says "nas.local still has 2 reads that have not returned" instead of starting more. Honest limit:
  an abandoned thread holds its stack and the mount until the kernel returns, possibly until the share is unmounted;
  on macOS a thread stuck on a hard NFS mount can also delay the app's exit (the agent exits with `_exit` after its
  shutdown deadline rather than joining it).
- **Picker reachability.** Each network volume's `statfs` runs on such a worker with a 10 s limit, off the UI path
  (the picker opens with the last known values); a volume that does not answer shows "Not answering" with the warning
  glyph and stays selectable (the scan then shows the same state at once).
- **A disconnect is a recoverable error, not a crash.** The walker maps the OS errors (`ENOTCONN`, `ETIMEDOUT`,
  `EHOSTDOWN`, `ESTALE`, `ENXIO`; Windows `ERROR_NETNAME_DELETED`, `ERROR_BAD_NETPATH`, `ERROR_UNEXP_NET_ERR`,
  `ERROR_DEV_NOT_EXIST`) to one typed `VolumeUnreachable`. The job ends `failed` with that cause; the index built so
  far is kept (finished folders are final, unfinished ones are marked incomplete); **Retry** re-walks only the
  incomplete folders. A share that stops answering without an error is shown by `pd-job` as "Still working: nas.local
  has not answered for 40 s" with Cancel; the UI never times out (ER-19).
- **What a share can't report is labelled, not guessed.** Purgeable space and snapshots are "Not reported" for SMB,
  NFS and AFP; free space is "not reported" when `statfs` gives zero blocks; sizes are logical when the server reports
  no allocation.
- **Hidden space is per volume type**: APFS purgeable, local snapshots and the other volumes of the container only on
  APFS (on the startup disk the sealed System volume is one of them, section 9.14); on NTFS the page file, hibernation file, swap file, Recycle Bin and shadow-copy storage; on ext4/XFS the blocks
  reserved for root; btrfs and ZFS snapshots "present, not measured"; FAT and exFAT have none.
- **A folder** has no hidden space and no volume scale (indeterminate progress, section 5); its capacity strip shows the
  folder's size and the free space of its volume.

## 9. Engine API (what `DiskUsage` has, what it needs)

**Today** (`dyno-core::protocol::Request::DiskUsage { path }`, `dyno-agent/src/cleanup/usage.rs`): one synchronous
request; a pruned `SizeNode { name, path, bytes, children }` tree, 3 levels, 12 children per folder, at most 200,000
entries or 5 s; logical sizes (`metadata.len()`); a fake child named "More not counted…" when a bound is hit;
`device()` is always 0 on Windows, so the walk does not stay on the volume there; links are skipped, hard links are
counted once per name. It lives in the cleanup module that ADR 0021 drops.

**Gaps, in priority order** (all queries bounded per ADR 0018, every cut carrying `Truncated`, every limit a
`LimitExceeded` per ADR 0019):

1. **A scan job, not a request** (ADR 0024 rule 3). `StartDiskScan { target: Volume(id) | Folder(path) }` returns a job
   id at once; `paddock-jobs` key `disk-scan:<canonical target>` (a second click returns the running job); phases
   `Reading volume facts` (instant), `Reading folders` (unit bytes, total = used − hidden for a volume, unknown for a
   folder or a share), `Checking duplicate hints` (unit bytes of the samples, total known); Cancel in every phase;
   `Background` class; per-device parallelism as in section 8. **Cancel latency:** the UI and the job state return within
   250 ms in every case (ADR 0008); the walk itself stops within 250 ms on local disks, while a worker blocked in a
   network syscall is abandoned (section 8) rather than awaited.
2. **Streaming partial results.** A `paddock://disk-scan` event (or `JobSnapshot` payload) of deltas, at most 4 per
   second and 2,000 nodes each: `ScanDelta { scan, seq, nodes: [NodeDelta { id, parent, name, kind, bytes, items,
   complete }] }`. Bytes and items only grow; `complete` flips once. The UI keeps the highest `seq`.
3. **An index the agent keeps, and children on demand.** The scan builds a compact arena (folders always; files only
   above the "notable" size, below that folded into their folder), bounded by the Setting `disk.scan.folders`
   (default 2,000,000, hard 5,000,000; about 120 MB at the hard bound). Queries:
   `DiskChildren { scan, node, depth: 1..=4, limit: ≤ 256 per parent, min_fraction }` → `{ node, children (nested to
   depth), folded: { count, bytes, items }, truncated }`; `DiskPath { scan, node }` → the full path (paths are not sent
   per node). Pre/post-order numbers on nodes make "is under" O(1) for the file lists.
4. **Richer nodes.** `id`, `kind` (`Folder`, `File`, `Package` (a macOS bundle counted as one item), `MountPoint`,
   `Unreadable`, `Placeholder` (cloud online-only)), **allocated bytes** (`st_blocks × 512`; Windows
   `GetCompressedFileSizeW`; logical size shown only where allocation is unknown), `items` and `folders` (recursive),
   `newest_mtime`, `complete`. Hard links counted once per (volume, file id). APFS clones share blocks but cannot be
   detected cheaply, so each copy is counted (stated in Help). Windows reads directories with
   `GetFileInformationByHandleEx(FileIdBothDirectoryInfo)`, which gives the allocation size and the file id per entry
   in one call (no extra open per file) and so finds hard links; Linux uses `statx` (`stx_blocks`, `stx_ino`).
5. **Volumes.** `Volumes` → `[Volume { id, name, mount, fs, location: Internal | External { bus } | Network { protocol,
   host }, capacity, free, rotational, read_only }]` plus a `paddock://volumes` event on mount/unmount. Per OS:
   - macOS: `getmntinfo(MNT_NOWAIT)` (type, flags; skip `MNT_DONTBROWSE`), DiskArbitration
     (`DADiskCopyDescription`: `DeviceInternal`, `DeviceProtocol` for USB/Thunderbolt, volume name; `DARegisterDiskAppeared/DisappearedCallback`
     for the event), `statfs` for capacity and free.
   - Windows: `GetLogicalDrives` + `GetDriveTypeW` (fixed, removable, remote, CD-ROM, RAM disk), `GetVolumeInformationW`
     (file system, label, serial: the serial is what keeps the walk on one volume), `GetDiskFreeSpaceExW`,
     `IOCTL_STORAGE_QUERY_PROPERTY` (bus type, seek penalty for rotational), `WNetGetConnectionW` for a mapped drive's
     `\\server\share` (and `WNetOpenEnumW` for connected shares without a letter); `WM_DEVICECHANGE` / `RegisterDeviceNotification` for the event.
     Junctions and other reparse points are not followed (`FILE_ATTRIBUTE_REPARSE_POINT`); placeholders
     (`FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`) count their allocation, not their size.
   - Linux: `/proc/self/mountinfo` (device numbers, file system, source `//host/share` or `host:/path`), `statvfs`;
     btrfs subvolumes have their own `st_dev`, so "stay on the volume" compares the file system (`f_fsid` / the
     mountinfo super block) rather than `st_dev`, and a subvolume is walked as part of its file system;
     `/sys/dev/block/M:m` for removable, rotational and the USB bus; pseudo file systems skipped; `inotify` on
     `/proc/self/mountinfo` (it signals `POLLPRI` on change) for the event.
6. **Hidden space per volume.** `VolumeSpace { volume, capacity, free, parts: [HiddenPart { kind, bytes: Option<u64>,
   detail }], remainder }` with `kind` one of `Purgeable`, `Snapshots { count }`, `OtherVolumes { [name, role, bytes] }`,
   `PageFile`, `Hibernation`, `RecycleBin`, `ShadowStorage`, `RootReserved`, `NotReported { reason }`. `remainder` =
   used − counted − measured parts, labelled "Snapshots and files Dyno can't read", and known only when the walk
   ends (`remainder: Option<u64>`, `None` before). Per OS:
   - macOS (APFS only): purgeable = `NSURLVolumeAvailableCapacityForImportantUsageKey` − free; other volumes of the
     container from `diskutil apfs list -plist` (`CapacityInUse` per volume; external tool, with a timeout); local
     snapshots from `tmutil listlocalsnapshots <mount>` (count and dates; macOS gives no per-snapshot size, so their
     bytes stay in `remainder`).
   - Windows (NTFS): `pagefile.sys`, `hiberfil.sys`, `swapfile.sys` sizes from their directory entries (no open);
     `SHQueryRecycleBinW` per drive; shadow-copy storage needs administrator rights, so it is read by the read-only
     helper or shown as "Needs administrator" (ADR 0021 keeps the helper read-only).
   - Linux: `f_bfree − f_bavail` (reserved for root); btrfs/ZFS snapshots `NotReported { "not measured" }`.
   - Network file systems: `NotReported` for purgeable and snapshots; free space as the server reports it.
7. **File lists.** `DiskFiles { scan, under: node, order: Largest | Oldest, limit ≤ 500, min_bytes }` over the notable
   files (≥ 10 MiB by default, a Setting; at most 200,000 kept, with `Truncated`). "Old" = not modified for a year
   (a Setting), sorted by size within that.
8. **Duplicate hints.** While walking, files ≥ 1 MiB are grouped by exact size; in the `Checking duplicate hints` phase
   (after the walk, only for a whole scan) the agent reads a 64 KiB sample at the start, middle and end of each
   candidate (streaming hash, ADR 0008 §5, BLAKE3) and groups equal samples. On a network volume sampling is capped
   (at most 2,000 candidates and 256 MiB read per scan, a Setting) and the cap is reported (`Truncated`).
   `DiskDuplicates { scan, under, limit ≤ 200 }` → `[DuplicateHint { bytes_each, count, space_if_true_copies, paths ≤
   20, basis: "same size and sampled content" }]`. `space_if_true_copies` = bytes_each × (count − 1), shown as
   "3.1 GB if they are true copies": APFS clones and hard links already share their blocks, so the real saving may be
   zero, and the UI says so. They are always labelled **hints**; a **Check this group** action hashes the whole files
   as a separate job before the UI ever says "identical". Nothing offers to delete a copy (ADR 0021).
9. **Reveal and copy, by node, not by path.** `RevealNode { scan, node }` (the agent resolves and re-checks the path,
   then asks the UI shell to reveal it: `NSWorkspace activateFileViewerSelectingURLs`, `SHOpenFolderAndSelectItems`,
   `org.freedesktop.FileManager1.ShowItems`); the UI never passes an arbitrary path (least privilege). Copy path uses
   `DiskPath` and the clipboard helper.
10. **Typed truncation.** The "More not counted…" child goes; `Truncated` and an `unreadable { folders, first: [path ≤ 10] }`
    count replace it.
11. **Windows stays on one volume** by comparing volume serial numbers, not `dev` (today `device()` is 0 there).
12. **Move out of `cleanup`.** `usage.rs` becomes `dyno-agent/src/disk/` (walk, index, volumes, hidden space) with a
    pure core in `dyno-core::disk` (index, folding, deltas, duplicate grouping), since ADR 0021 drops cleanup.
13. **Full Disk Access detection (macOS).** `Permissions` → `{ full_disk_access: Granted | Denied | Unknown }`, probed
    without prompting by reading a TCC-protected path that always exists (`~/Library/Safari` metadata or the TCC
    database's directory) and treating `EPERM` as denied; re-probed when the app becomes active, so Retry after the
    user's change works without a restart. Windows and Linux return `NotApplicable`; their unreadable folders come from
    the walk (`unreadable`).
14. **macOS sealed System volume and firmlinks.** `/` is the read-only sealed System volume; user data lives on the
    Data volume (`/System/Volumes/Data`), joined to `/` by firmlinks (`/Users`, `/Applications`, `/Library`,
    `/private`, `/opt`, `/usr/local` and the others listed in `/usr/share/firmlinks`). A walk of "Macintosh HD" walks
    the **Data volume** and presents the firmlinked folders under their usual names; it does not walk `/usr`, `/bin`,
    `/sbin` or `/System` on the System volume, whose size is counted once, as "Sealed System volume" in hidden space
    (`diskutil apfs list`). The "stay on the volume" check therefore uses the Data volume's device, and
    `/System/Volumes/Data` itself is not walked a second time under its own path. The mockups follow this (`usr`
    shows only `local`).

Out of scope for 1.0: incremental rescans from FSEvents/USN/fanotify, colour by file kind, scans on a schedule.

## 10. Tests (when built)

- Engine: walk over generated trees (never the user's disk, ER-14; sizes and counts as parameters, one cheap test pins
  the real constants); monotone deltas; cancel within 250 ms per phase on a local fake; a fake network port whose calls
  block forever (Cancel returns at once, the worker is abandoned, the abandoned count bounds new scans); the remainder
  appears only at the end and the parts sum to used; disconnect simulated by a fake file system
  port returning `ENOTCONN` mid-walk, partial index kept, Retry walks only incomplete folders; per-host parallelism
  bound; hidden-space parsers over recorded `diskutil`/`tmutil` output with invented values; Windows serial check type-checked
  on the Windows runner.
- UI: `pd-sunburst` unit and gallery tests (spec); Dyno page specs per state with the mock backend; Playwright
  screenshots of every state at 1440 x 900, 2x, both themes (UI rule 20); axe clean; keyboard path test (tree and
  treegrid); reduced-motion test (no tween frames).
