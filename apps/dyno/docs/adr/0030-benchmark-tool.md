# 0030. The benchmark tool: per-drive disk, CPU with a sustained run, memory bandwidth and latency, GPU through wgpu

- Status: Proposed (2026-11-03). Numbered 0030: 0029 is left for the parallel cached-views ADR (README: numbers are
  allocated at merge time; renumber on a clash). Revised 2026-11-03 after an independent review (verdict: changes; the
  six findings confirmed; two P1 and eight P2 taken into the text, marked "review").
- Date: 2026-11-03
- Related: amends ADR 0015 (benchmarks on click: the process model, the disk test's size and cleanup, "GPU reported
  unavailable"); ADR 0007 (tier 3 plugins; 0015 already chose a first-party child process over a plugin, kept here);
  ADR 0018 (the throttling tracker, reused for the sustained run); ADR 0021 (monitoring only: the scratch file is the one
  thing Dyno writes, and only in its own folder); ADR 0023 (shell dialogs); Paddock ADR 0008 (`paddock-jobs`), ADR 0041
  on `origin/docs/redesign-adrs-2-4-6` (proposed rule 60, tests control time); rules 16, 52, 53; ER-14, ER-17 to ER-20,
  ER-23, ER-27.
- Evidence marks: **[S]** read in this repository at `origin/main` a18e18801; **[M]** measured on the M4 Pro Mac mini
  (Mac16,11, 14 cores, 64 GB, macOS 27.2, internal SSD, load average about 23 from other agents, 2026-11-03) with a
  synthetic 256 MiB file in the session scratchpad, never a user file; **[R]** read in an external source (URL and date
  below); **[U]** secondary or unverified, to be checked in the named work package; **[I]** inference.

## Context

The user asked (2026-11-03) for a benchmark tool in Dyno 1.1 measuring disk speed (sequential and random read and write
per drive), CPU (single and multi core, a sustained load that shows throttling), memory (bandwidth and latency) and GPU
(basic compute and graphics throughput, per-OS APIs).

### What exists [S]

- `crates/dyno-bench` (one binary, about 300 lines): `cpu.rs` runs a xorshift-multiply mix for one time budget on one
  thread and on `available_parallelism()` threads; `memory.rs` copies 64 MiB with `copy_from_slice` and chases a
  Sattolo cycle of `u32` indices over 64 MiB; `disk.rs` writes a 16 to 512 MiB file (default 128 MiB, constant byte
  0x5A) with `sync_all`, reads it back sequentially and does random 4 KiB reads, all through the page cache, and labels
  the reads "may be cached"; GPU returns `Unavailable`. Output is one JSON status on stdout.
- `dyno_core::bench` (648 lines): `BenchKind`, `BenchMetric`, `BenchStatus`, `RunPlan` validation, `LoadAtRun`
  (CPU and memory percent, on battery), `compare` with a fixed 3 % noise band and warnings for a different machine, load
  or power source, `BenchHistory` (versioned JSON, 50 runs, damage tolerant).
- `dyno-agent/src/bench.rs` (419 lines): `BenchCentre` runs one kind at a time on a thread through the `BenchLauncher`
  port, kills a child after 60 s (180 s for disk) via `dyno_platform::process::run_with_timeout`, makes and removes the
  scratch folder under `std::env::temp_dir()`, and keeps the measuring thread at normal priority on purpose (its comment
  cites rule 52). Requests `benchmarks`, `startBenchmark`, `cancelBenchmark`, `deleteBenchmark`, `compareBenchmarks`.
- UI `app/src/app/features/benchmarks`: kind checkboxes, run name, a before/after stepper, a step list while running,
  the latest result, a comparison table and the saved-runs table.
- The plugin tier (ADR 0007) is not used; ADR 0015 rejected it for first-party kernels. This ADR keeps that.

### What is wrong with it [S][M]

1. **The runner is not shipped.** `tauri.conf.json` `bundle.externalBin` is `["binaries/dyno-agent", "binaries/dyno"]`
   and `scripts/check-shipped.mjs` `SHIPPED` is `['dyno-app', 'dyno-agent', 'dyno-cli']`. `ProcessLauncher::beside_agent`
   looks for `dyno-bench` next to the agent, so in an installed 1.0 every kind ends "Unavailable: ... missing from the
   installation; reinstall Dyno", and reinstalling cannot help. Root cause: the bundle list was never extended when
   WP-11 landed (6d07868ff); no check asserts that every executable the agent starts is bundled.
2. **The disk reads measure memory.** Measured with the same pattern (write, then read the file just written) on the
   internal SSD [M]:

   | Pattern | Written through the cache, read cached | Written through the cache, read with `F_NOCACHE` | Written and read with `F_NOCACHE` |
   |---|---|---|---|
   | Sequential read, 1 MiB, 1 thread | 16,949 to 17,455 MB/s | 16,669 to 17,064 MB/s | 2,406 to 2,434 MB/s |
   | Random read, 4 KiB, 1 thread | 852k to 893k IOPS | about 704k IOPS | 8,742 to 9,367 IOPS |
   | Sequential write, 1 MiB, `F_FULLFSYNC` at end | 2,161 to 2,465 MB/s | | 1,598 to 2,069 MB/s |

   Three repetitions each. The cached numbers are RAM, about 7x and 95x the device. Note the middle column: turning
   caching off for the reader alone is not enough on macOS, pages written through the cache are still served from it;
   the file must be written uncached too (or its pages evicted). The write constant 0x5A is compressible, which some SSD
   controllers exploit [R: CrystalDiskMark notes the result "depends on test data (random, 0fill)"].
3. **One drive only.** The scratch folder is `std::env::temp_dir()`, the system drive.
4. **Cancel waits for the running kind.** `cancel` sets a flag checked between kinds; the child runs to its end or its
   timeout. ER-18 asks that Cancel works in every phase.
5. **No progress inside a kind, no repeats, no spread.** One sample per metric; the comparison's noise band is a
   constant, not the runs' own variation.
6. **Linux runs it inside the agent's limits.** The agent's systemd user unit (`dyno-agent/src/service/spec.rs`) sets
   `Nice=10`, `CPUWeight=20`, `IOSchedulingPriority=7` and `MemoryHigh=256M`; a child inherits the cgroup and the nice
   value, so a memory test with more than 256 MiB of arrays is reclaimed under it and any contention halves the CPU
   share [I from the unit text; to be measured in WP-B2].
7. **Memory latency understates.** Indices are packed `u32`s in one array, so 16 consecutive slots share a 64-byte line;
   a random cycle still touches a fresh line on most steps, but the method differs from the usual one-node-per-line chase
   (lmbench `lat_mem_rd`) and has no size sweep to show the cache levels.
8. **Tests run real CPU burners for real time** (`cpu::tests::a_short_run_reports_three_positive_metrics`, `disk` and
   `memory` tests with millisecond budgets). Proposed rule 60 (ADR 0041) forbids tests that depend on host speed.

### Sources (read 2026-11-03) [R]

- fio documentation, https://fio.readthedocs.io/en/latest/fio_doc.html: `direct` "use non-buffered I/O. This is usually
  O_DIRECT" (not on OpenBSD or ZFS on Solaris; not with the synchronous engines on Windows); `ramp_time` runs the job
  "before logging any performance numbers"; `invalidate` drops the file's cached pages before I/O (default true);
  `iodepth` default 1. fio is GPL-2.0: its method is used, never its code.
- STREAM, https://www.cs.virginia.edu/stream/ref.html: each array at least 4x the sum of the last-level caches (or 1
  million elements); kernels COPY (16 bytes per element), SCALE (16), ADD (24), TRIAD (24); write-allocate traffic not
  counted; best of typically 10 trials reported. Results may be called "STREAM benchmark results" only when they follow
  the run rules; others must be labelled. So Dyno reimplements the method in Rust and labels it "STREAM-style".
- lmbench `lat_mem_rd`, https://github.com/intel/lmbench/blob/master/doc/lat_mem_rd.8: a ring of pointers walked with
  dependent loads over array sizes from 512 bytes up, reported in ns per load; strided rings are distorted by
  prefetchers, hence a random ring here. lmbench's licence is not permissive enough to copy; method only.
- CrystalDiskMark, https://crystalmark.info/en/software/crystaldiskmark/crystaldiskmark-main-menu/: default tests SEQ1M
  Q8T1, SEQ1M Q1T1, RND4K Q32T1, RND4K Q1T1; test data random or 0-fill. Default size 1 GiB and 5 runs, sizes 16 MiB to
  64 GiB, "may be too large for low speed storage (USB memory etc.)" [U: from the vendor's site as quoted by a search
  summary, https://crystalmark.info/en/?p=265, not opened directly].
- Geekbench 6 internals, https://www.geekbench.com/doc/geekbench6-benchmark-internals.pdf: a "shared task" multi-core
  model, a gap between workloads to limit thermal effects (raised from 2 s to 5 s in 6.1), subsection scores as geometric
  means [U: the PDF could not be text-extracted here; taken from search summaries of it and from
  https://www.techpowerup.com/309820/primate-labs-rolls-out-geekbench-6-1]. Proprietary: ideas only.
- Linux `open(2)`, https://man7.org/linux/man-pages/man2/open.2.html: `O_DIRECT` may impose alignment on buffer address,
  length and offset; query it with `statx` `STATX_DIOALIGN` (Linux 6.1+); some filesystems fail `open` with `EINVAL`.
- Windows file buffering, https://learn.microsoft.com/en-us/windows/win32/fileio/file-buffering:
  `FILE_FLAG_NO_BUFFERING` needs sizes and offsets in whole sectors and sector-aligned buffers; use the physical sector
  size from `IOCTL_STORAGE_QUERY_PROPERTY`; `VirtualAlloc` memory is page aligned.
- macOS `fcntl(2)` (local man page, macOS 27.2): `F_NOCACHE` "turns data caching off"; `F_FULLFSYNC` asks the drive to
  flush to permanent storage.
- wgpu, https://github.com/gfx-rs/wgpu: MIT OR Apache-2.0; first-class backends Vulkan and DX12 on Windows, Vulkan on
  Linux, Metal on macOS; MSRV 1.87 (Dyno's is 1.89).
- Apple, "Tuning your code's performance for Apple silicon",
  https://developer.apple.com/documentation/apple-silicon/tuning-your-code-s-performance-for-apple-silicon: background QoS
  makes the system "more likely" to run work on efficiency cores [U: secondary sources such as
  https://eclecticlight.co/2025/05/09/what-is-quality-of-service-and-how-does-it-matter/ say E cores only].
- Apple `ProcessInfo.thermalState` (nominal, fair, serious, critical) and `isLowPowerModeEnabled`
  [U: the page did not render for the fetcher; from general knowledge, verify in WP-B2].
- Windows power modes: `PowerGetUserConfiguredACPowerMode` / `DCPowerMode` and `GUID_POWER_MODE_BEST_PERFORMANCE`
  (Windows 11) [U: from a search summary of learn.microsoft.com powrprof pages; Dyno's facts already read a power plan].

## Decision

### 1. Method per test

All kernels are first-party Rust, safe code, `std` plus the dependencies named below (all MIT or Apache-2.0). Each kernel
is split into a **fixed-work function** (sizes and counts in, a checksum and the bytes or operations done out; pure and
deterministic) and a **timing driver** that repeats it against an injected clock until a time or byte budget is met
(section 5).

**Disk (per drive).**
- *macOS permission (review):* writing under `/Volumes/*` needs the Removable Volumes or Network Volumes privacy
  permission (TCC), which is granted per process. The child `dyno-bench` is started by the agent, so the agent's
  identity is the one asked [I: which process TCC attributes a sidecar's access to is checked in WP-B3 on a clean VM].
  Dyno never triggers the prompt in the middle of a run: the pre-flight probes the chosen folder (create and remove an
  empty file in Dyno's own folder) before the run starts, so a prompt appears while the person is at the picker. A
  denial ends that drive's test before anything is written, as Unavailable: "macOS did not allow Dyno to write to
  <volume>. Allow Dyno in System Settings > Privacy & Security > Files and Folders, then run again", with a button that
  opens that pane; the other drives still run.
- *Where:* the user picks drives from Dyno's volume list (`dyno_platform::volumes`). The agent makes its own folder
  `<volume>/.dyno-bench-<run-id>` (or, on the system drive, under the temp directory; on a drive where the root is not
  writable, a folder the user picks through the shell's folder dialog, canonicalised and checked to be on that volume by
  device id). Refused, with the reason: read-only, network (SMB, NFS, AFP), optical and RAM disks; network drives can be
  enabled per run with a confirmation, results marked "network".
- *Never a user file, cleanup guaranteed:* the file is created with `create_new` inside Dyno's own folder; on Unix it is
  unlinked right after opening (the open descriptor keeps it; the space returns when the process ends, even if killed);
  on Windows it is opened with `FILE_FLAG_DELETE_ON_CLOSE`. The agent removes the folder after the child exits and
  records its path and file identity before starting, so a crash of the agent itself is cleaned up on the next start by
  identity, never by name alone (rule 53).
- *Free space:* refused unless free space is at least the file size plus the larger of 10 % of the volume and 2 GiB;
  re-checked before each write pass; `ENOSPC` / `ERROR_DISK_FULL` ends the kind cleanly as Failed with the reason.
- *Bypassing the cache:* Linux `O_DIRECT` with alignment from `STATX_DIOALIGN` (fallback 4 KiB); Windows
  `FILE_FLAG_NO_BUFFERING | FILE_FLAG_WRITE_THROUGH`, buffers from `VirtualAlloc`, sizes in physical sectors; macOS
  `F_NOCACHE` on the writer **and** the readers (finding 2) and `F_FULLFSYNC` at the end of each write pass. Where the
  file system refuses direct I/O (Linux `EINVAL`, ZFS), the test runs buffered, evicts with `posix_fadvise(DONTNEED)`
  after an `fdatasync`, and every metric of that drive carries "cached reads possible".
- *Patterns* (CrystalDiskMark's defaults as the reference, fio's names in the logs): sequential 1 MiB with 1 thread and
  with 8 threads, random 4 KiB with 1 thread and with 32 threads (each thread one outstanding I/O with `pread`/`pwrite`
  on its own offsets: portable, labelled "8 threads", never "queue depth 8", since macOS has no usable async file I/O).
  Each pattern: read and write. Data is random bytes from a fixed-seed generator (incompressible).
- *Ramp and budget (review):* each pattern starts with a ramp that is not counted (fio `ramp_time`), limited by **bytes
  and time, whichever ends first**: at most 1/8 of the file size or 1 s. A time-only ramp would write about 7 GB per
  pass on a fast NVMe drive. The measured part then runs for up to 5 s or until it has moved the file's size, whichever
  comes first. Every byte limit is enforced in the child's loop, never derived from a speed.
- *Wear (review):* default file 1 GiB, 3 repetitions, 4 write patterns. Bytes written per drive and run are counted in
  full by a pure function, `dyno_core::bench::disk_write_budget(plan) -> u64`: the **fill** (one pass of the file size,
  so the read patterns read real data), plus per write pattern and repetition the **ramp** (at most 1/8 file) and the
  **measured part** (at most one file). Default: 1 GiB fill + 12 x 1.125 GiB = **14.5 GiB at most per drive**. The
  picker's "This writes up to 15 GB to Macintosh HD" is that function's result rounded up, and the child enforces the
  same budget (it stops a pattern that would exceed it, logged), so the shown number is an upper bound by construction.
  Unit tests pin the estimate for each preset and size, and a property test checks that a scripted child that writes
  as fast as allowed never exceeds it. For scale, a consumer 1 TB TLC SSD is commonly rated a few hundred TBW [U:
  typical datasheet figures, not checked per model], so one run is on the order of 0.003 %. Choices: 256 MiB (quick),
  1 GiB (default), 4 GiB. A **sustained write** test (past the drive's SLC cache, up to the smaller of 25 % of free
  space and 64 GiB) is off by default and needs its own confirmation that names the amount; it is counted by the same
  function. Hard disks (rotational, from media traits) get 2 s random patterns.
- *Reported:* MB/s for sequential, IOPS and MB/s for random, plus the drive model, bus, file system and free space.

**CPU.**
- *Kernels:* integer (the existing mix plus a branchy sort of a 4 MiB `u32` array, so branch prediction counts), float
  (a blocked `f64` matrix multiply, scalar order fixed so the checksum is exact), SIMD (an `f32` blocked multiply and a
  dot product compiled for the widest vector unit present with run-time dispatch: NEON on aarch64 as baseline,
  AVX2+FMA and AVX-512 on x86_64 through the `multiversion` crate [U: licence MIT OR Apache-2.0 to confirm with
  cargo-deny in WP-B4]; the result names the width used).
- *Single and all cores:* single is one thread; multi is one thread per logical CPU, each with its own data (the
  "separate task" model: Dyno measures what the silicon can do, not how an app scales; the label says so). Scaling =
  multi / single, also shown.
- *Sustained run:* all-core float kernel for a chosen length (2, 5 default, 10 or 20 minutes). The child emits throughput
  once per second; the agent's sampler adds frequency, the hottest temperature with its critical value, the OS thermal
  state (macOS `thermalState`, Linux thermal zones and `scaling_cur_freq`, Windows what the counters give) and power
  source. The trace runs through `dyno_core::throttle::ThrottleTracker` (ADR 0018), so an episode found here is judged by
  the same rules as in monitoring. Reported: peak (best 10 s window in the first 30 s), sustained (median of the last
  60 s), sustained / peak in percent, time until throughput first fell more than 10 % below peak, and the episodes with
  their causes. Readings Dyno cannot get without the helper (Apple silicon frequency) show as Unavailable with the
  reason; the throughput trace never depends on them.
- *Thermal safety stop:* the run stops (state Stopped, reason named) when the hottest sensor is within the throttle
  tracker's hot margin of its critical value for 10 s, when macOS reports `critical`, or when the machine switches to
  battery without consent. A stop keeps the trace so far.
- *No readable sensor (review):* where Dyno reads no temperature and no OS thermal state (many Windows desktops without
  the helper, some Linux machines, VMs), Dyno cannot stop on heat; the protection is then the firmware's own throttling
  and shutdown. In that case the picker says so ("Dyno cannot read this computer's temperature; the hardware protects
  itself") and the sustained run is limited to 5 minutes (the 10 and 20 minute choices are disabled with that reason).

**Memory.**
- *Bandwidth:* STREAM-style Copy, Scale, Add and Triad on `f64` arrays, single thread and all threads (each thread a
  contiguous slice). Array size: at least 4x the sum of the last-level caches (read from `sysctl hw.perflevel*`
  on macOS, `/sys/devices/system/cpu/cpu0/cache` on Linux, `GetLogicalProcessorInformationEx` on Windows; 32 MiB per
  cache when unknown), at least 128 MiB per array, at most 1/8 of the memory available at start; refused with a reason
  when that leaves less than 4x. Bytes counted as STREAM counts them; label "STREAM-style", never "STREAM results".
- *Latency:* a random cyclic chain with one node per 128-byte line (covers 64 and 128-byte lines), walked with dependent
  loads, over a sweep from 16 KiB to 512 MiB (log steps); the headline is the 256 MiB point ("main memory"), the sweep is
  shown as a chart so the cache levels are visible. Normal 4 KiB/16 KiB pages; no huge pages (said in the details).

**GPU.**
- *API:* `wgpu` (MIT OR Apache-2.0) with backends Metal (macOS), DX12 then Vulkan (Windows), Vulkan (Linux); no GL
  backend (it adds size and its timings are not comparable). Every adapter is listed (integrated and discrete); the
  default is the high-performance one; the user may pick another.
- *Compute:* an FMA throughput kernel (each invocation a long dependent chain of `vec4<f32>` fused multiply-adds, enough
  workgroups to fill the device; GFLOPS) and a storage-buffer copy (GB/s, buffer size within the adapter's
  `max_storage_buffer_binding_size`).
- *Graphics:* off-screen 3840x2160 RGBA8: fill rate (full-screen triangles with blending, Gpixel/s) and geometry
  (instanced small triangles, Mtri/s). Nothing is shown on screen.
- *Dispatch length (review):* every submission is sized to take about 100 ms and never more than 250 ms, and a
  measurement repeats submissions instead of making one long one. Windows resets a GPU whose work runs past the TDR
  limit (about 2 s by default), and macOS has its own GPU watchdog; a reset would take the user's display with it. The
  size is found from a short calibration submission and halved whenever one takes over 250 ms.
- *Timing:* timestamp queries where the adapter supports them, else wall-clock around submit and the queue's completion,
  summed over submissions until at least 1 s of work is measured. The first submissions (shader compile, clocks
  ramping) are warm-up and not counted.
- *Honesty:* an adapter of type CPU (llvmpipe, WARP, any software rasteriser) is reported as "software renderer, not a
  GPU", never compared with hardware runs; no adapter gives Unavailable with the reason.

### 2. Process model

- **Separate processes.** `dyno-bench` (disk, CPU, memory) and `dyno-bench-gpu` (wgpu only, so the large graphics
  stack and driver crashes or hangs stay out of the other kinds and out of `dyno-bench`'s size). Both are bundled as
  sidecars, signed with identifiers under `net.decryptology.dyno.` (the sidecar rule `check-bundle.sh` enforces), and listed in
  `check-shipped.mjs` and `check-bundle.sh` (finding 1).
- **Protocol.** The child writes JSON lines on stdout: `progress` (phase id, label, repetition n of m, a count with a
  known or unknown total), `sample` (sustained run, once a second), then exactly one `result`. Parsed once at the edge
  into typed values (`dyno_core::bench::wire`), with line and count limits; an unknown or malformed line ends the kind
  as Failed with the line number, never a panic.
- **Off the UI thread, a job (ER-17, ER-18).** A run is a `paddock-jobs` job in the agent, relayed to the app with
  `paddock_jobs::wire`; `startBenchmark` returns at once. Phases are labelled "Disk · Macintosh HD · Random write, 32
  threads · pass 2 of 3" and each has its own scale; the overall step list stays. Progress never goes backwards.
- **Cancel in every phase.** The child runs under `paddock_proc::Supervised`; Cancel ends its process tree at once, the
  kind is Stopped, the rest are "Cancelled before it started", the scratch file vanishes with the descriptor and the
  agent removes the folder.
- **No UI timeout, no quiet kill (ER-19, review).** The fixed 60 s / 180 s kill is replaced by two signals, neither of
  which ends the child by itself:
  - a **heartbeat** written by a separate thread in the child every second (independent of the measuring threads, so a
    long kernel step or a slow I/O never looks like a hang, and a stuck process does stop beating);
  - a **plan bound**: the sum of each phase's budget, with disk phases sized by their byte budget at a pessimistic
    10 MB/s (a slow USB stick) and GPU phases by their submission count at the 250 ms cap, plus 50 %.
  When the heartbeat stops for 30 s or the bound passes, the job shows "Not responding" with Cancel (and "Still
  working" while the heartbeat continues past the bound); the person decides. The only automatic end is the safety
  stop (heat, unplugged without consent), which is named as such.
- **Linux: a named unit, stopped on Cancel (review P1).** On Linux the child is started outside the agent's cgroup and
  nice value as a transient service of the user manager:
  `systemd-run --user --unit=dyno-bench-<run-id> --collect --wait --pipe --service-type=exec -- <runner> ...` (nice 0, no
  `MemoryHigh`). Killing the `systemd-run` client does not stop the unit, so Cancel, the "Not responding" Cancel, a safety
  stop and the agent's shutdown all run `systemctl --user stop dyno-bench-<run-id>.service` and wait for the unit to be
  inactive. The agent records the unit's `InvocationID` (from `systemctl --user show -p InvocationID`) with the run
  before measuring starts; on its next start it stops any `dyno-bench-*` unit whose invocation id matches a record, and
  only those (rule 53: never by the name alone, a reused name could be someone else's). The WP-B2 spike proves, before
  any kernel work: the unit gets nice 0 and no memory limit; Cancel stops an all-core burner within 2 s; killing the
  agent mid-run and restarting it stops the leftover unit; a unit with a matching name but a foreign invocation id is
  left alone. Without a user manager the child runs inside the agent's cgroup as an ordinary supervised child, and the
  run records "limited: nice 10, memory limit 256 MiB" and is marked low confidence [I: the unit's nice value and
  limits are to be confirmed by that spike].
- **Priority: normal, an explicit exception to rule 52's gentle default (review).** Rule 52 and `ThreadRole::Bulk` run
  bulk work gently; a benchmark is the one bulk-sized job that must not, because its result *is* the speed of the
  machine, and a lowered thread measures the scheduler instead (on Apple silicon, likely the efficiency cores). It is
  also work the person started and waits for, which rule 52 already treats as not starvable. The exception is narrow:
  only `dyno-bench` and `dyno-bench-gpu`'s measuring threads, only after a click, never scheduled; the agent's
  orchestration thread stays at its normal role. In exchange: a pre-flight warns when other programs use more than 15
  points of CPU (the existing `LOAD_WARNING_POINTS`) and offers to wait; one run at a time; Cancel always works. The
  exception is recorded where conformance can see it: a `// rule 52 exception: ADR 0030` marker on the one place the
  runners set their priority, and a conformance note listing it as the only allowed exception (the check flags any
  other process started by the agent at normal priority for bulk work).
- **Battery and power.** Before the run: on battery, a dialog (shell dialog API, ADR 0023) "On battery. Results will be
  lower and the battery drains faster. Run anyway?" with consent for this run only; Low Power Mode or a power-saving mode
  is shown in the same pre-flight. During the run: a switch to battery without consent stops the run.
- **Logs (ER-27).** Per run: a correlation id, the plan, the pre-flight facts, each child's exact command, pid, exit
  code and stderr tail, each phase's start, duration and outcome, why it stopped, and that cleanup succeeded. No paths
  under the user's home in logs beyond the drive's mount point.

### 3. Reproducibility

- **Warm-up and repeats.** CPU and memory: 1 s warm-up not counted, then 5 repetitions with a 2 s gap between kernels
  (Geekbench's reason: limit heat carried from one test into the next). Disk: ramp as above, 3 repetitions. GPU: warm-up
  dispatch, 5 repetitions.
- **Median and spread, shared (ER-22, review).** Every metric stores all repetitions; the headline is the median, the
  spread is min to max and the coefficient of variation. A metric with a CV over 10 % is marked "unsteady". The
  statistics come from `paddock-bench` (`stats::Samples` and its `Summary`: n, min, median, p95, max; NaN and infinite
  samples ignored), extended there with the standard deviation and CV and tested once there; the timing drivers use
  `paddock_bench::Clock` (`ManualClock` in tests). Dyno adds no statistics code of its own.
- **State recorded with each run** (extends `LoadAtRun`): power source and charge, power mode (macOS Low Power Mode,
  Windows power mode or plan, Linux power profile when readable), thermal state and hottest temperature at start and end,
  other programs' CPU during the run (the agent's sampler, excluding Dyno's own processes), memory pressure, machine
  model, OS version, Dyno version, **method version per kind**, and for disk the drive model, bus, file system, free
  space and whether direct I/O was used; for GPU the adapter, backend and driver.
- **Comparison.** Only runs with the same method version and the same device (machine; drive; adapter) are compared; a
  change counts as better or worse only when it is larger than both the noise band and the two runs' spreads together
  ("about the same" otherwise). The existing warnings stay and gain power mode, thermal state and direct-I/O mismatch.
  Runs from 1.0's method are kept, shown as "measured the old way", and never compared with 1.1 runs.
- **History and export.** The bounded JSON history stays (schema version 2, damage tolerant, 200 runs). Export of a run
  or a comparison as JSON and CSV through the shell's save dialog, written by Rust.

### 4. UI

- **Picker:** presets Quick (about 1 minute: CPU, memory, system drive at 256 MiB), Full (about 6 minutes: all kinds,
  chosen drives at 1 GiB) and Sustained CPU (the chosen length); Custom shows kinds, drives (multi-select with model and
  free space) and GPU adapter. Below: the estimated time and the bytes it writes per drive. One primary button. The
  before/after stepper stays.
- **Live:** the job's phase label and scale, the step list, Cancel always enabled; the sustained run shows a live chart
  of throughput with frequency and temperature where available, and the stop reason when it stops.
- **Results:** per metric the median, the spread ("2,410 MB/s, 2,380 to 2,440"), "compared with your last run" (the last
  comparable run, with the verdict word and the change), the warnings that apply, and an "unsteady" or "low confidence"
  mark with its reason. No composite score (D4). Status is never colour alone.
- **History:** the existing table plus method version and device columns; compare two; export.

### 5. Tests (rule 60) and VM limits

- **Pure core, no real time:** kernels' fixed-work functions are tested for exact checksums at tiny sizes; timing
  drivers take `paddock_bench::Clock` and a `Pace`, so "stops at the budget", "ramp not counted", "the ramp stops at its
  byte limit" and "the gap is kept" are tested with `ManualClock`; the spread-aware verdict, plan validation, the plan
  bound, `disk_write_budget` (pinned per preset and size, and a property test that a scripted child writing as fast as
  allowed never exceeds it), the GPU submission sizing (halves after a slow submission, never plans one over 250 ms),
  free-space policy and the wire parser (limits, malformed lines, never panics; a fuzz target) are pure. Median, spread
  and CV are tested in `paddock-bench`, not again here.
- **Linux unit (WP-B2 spike, then a Linux CI test with a scripted runner):** the unit name and stop command are built by
  a pure function; Cancel, "Not responding" Cancel and agent shutdown each stop the unit; restart cleanup stops only
  units whose invocation id matches a record.
- **Heartbeat:** a scripted child that keeps beating but sends no progress shows "Still working"; one that stops beating
  shows "Not responding"; neither is ended without Cancel.
- **Disk safety with real files, never timed:** `create_new` refuses an existing file; the unlink-on-open (Unix) and
  delete-on-close (Windows) file is gone after the child is killed (a scripted child that is killed by the test);
  free-space refusal through a fake volume reader; network and read-only volumes refused; the agent's crash cleanup
  removes only a folder whose identity matches its record.
- **Agent orchestration** through the `BenchLauncher` port with `ScriptedChild` (ADR 0041): Cancel in each phase ends the
  tree and leaves consistent history; the heartbeat ends a silent child; a crash is one Failed kind; battery consent and
  the thermal stop are driven by a fake power source and a scripted sensor stream.
- **Shipping:** `check-shipped.test.mjs` asserts every executable that `dyno-agent` starts by name is in
  `bundle.externalBin` (the failing test below); `check-bundle.sh` asserts both runners start (`--version`) from the
  built app and carry the signing identifiers.
- **UI:** component tests against the mock backend for every state (empty, running each phase, stopped, unavailable,
  unsteady, software GPU, consent dialogs); one Playwright flow.
- **VMs measure mechanics, not numbers.** In `vm-smoke`, a `--plan smoke` run (tiny sizes, one repetition) checks that
  phases appear in order, every kind ends Done or Unavailable with a reason, results are finite and positive, no scratch
  folder or file is left, and on a VM without a GPU the GPU kind reports a software renderer or Unavailable. No test
  asserts a speed; speed of Dyno itself stays with `dyno-agent bench` and rule 16.

The failing test WP-B0 starts with (in `scripts/check-shipped.test.mjs`; it fails on `origin/main` because
`dyno-bench` is started by the agent but not bundled):

```js
test('every executable the agent starts is bundled', () => {
  const conf = JSON.parse(readFileSync(join(root, 'app/src-tauri/tauri.conf.json'), 'utf8'));
  const bundled = conf.bundle.externalBin.map((p) => p.split('/').pop());
  for (const exe of startedByAgent(root)) assert.ok(bundled.includes(exe), `${exe} is started by dyno-agent but not in bundle.externalBin`);
});
```

(`startedByAgent` reads the launcher names declared in one place in `dyno-agent`, so a new runner cannot be forgotten.)

### 6. Parallelism and cost (ER-20)

- Kinds run one after another, never in parallel: each measures a shared resource and would disturb the others.
- Inside a kind the parallelism is the measurement (threads per core, I/O threads), not a speed-up.
- Cost to the machine is bounded: time by the plan bound, disk writes by the wear budget shown before the run, memory by
  1/8 of available memory, heat by the safety stop. Cheaper: Quick is the default preset; disk repetitions reuse one
  file; the sweep for latency reuses one buffer.

## Consequences

- The benchmark page works in an installed build for the first time, with disk numbers that are the device's and not
  RAM's, per drive.
- The bundle grows by `dyno-bench-gpu` with wgpu [U: size not measured; WP-B6 measures it and the release notes state it].
- The 1.0 history cannot be compared with 1.1 runs (method versions).
- Apple silicon frequency stays Unavailable until the helper (ADR 0022/0027) reads it; the sustained run still shows
  throttling through throughput.
- On Linux without a systemd user manager, results carry the agent's limits and say so.

## Work packages

| WP | Scope | Agent | Days |
|---|---|---|---|
| B0 | Ship the runners: the failing test above, `externalBin`, `check-shipped`, `check-bundle`, signing ids; one launcher-name list. **Lands in 1.1.0 only, together with B3 (or with Disk turned off in the build); the 1.0 disk test is never shipped** | implementer | 0.5 |
| B1 | Core: method versions, repetitions through `paddock-bench` stats (add standard deviation and CV there, tested there), the state record, spread-aware comparison, history v2, export JSON/CSV, wire types and parser, `disk_write_budget`, plan bound | implementer | 2 |
| B2 | Process model: JSON-lines protocol with the child's heartbeat thread, `paddock-jobs` job and relay, `Supervised` Cancel, "Still working" / "Not responding", pre-flight (load, battery consent, power mode, TCC probe), thermal stop and the no-sensor limit, the rule 52 exception marker and conformance note, logs; **the Linux spike first** (named unit, `systemctl --user stop`, invocation-id cleanup) | designer review, implementer | 3.5 |
| B3 | Disk: drive picker data, scratch folder and identity record, unlink/delete-on-close, free space and refusals, macOS TCC handling, direct I/O per OS with alignment (checked on the Windows and Linux runners), the four patterns read and write with byte-bounded ramps, threads, random data, enforced write budget, sustained write | implementer (Opus review) | 4 |
| B4 | CPU: integer, float, SIMD kernels with dispatch, single and all cores, sustained run with throttle tracker and safety stop | implementer | 2.5 |
| B5 | Memory: cache-size discovery per OS, STREAM-style four kernels single and all threads, latency sweep | implementer | 1.5 |
| B6 | GPU: `dyno-bench-gpu`, adapters, compute FMA and bandwidth, fill rate and geometry, 100 ms submissions with calibration, timestamps or wall clock, software detection; runs on the Windows (DX12) and Linux (Vulkan) runners; bundle size measured | implementer (Opus review) | 4.5 |
| B6a | Signing and packaging of the new sidecar on three OSes: macOS signing ids, hardened runtime and notarization of the bundle with `dyno-bench-gpu`, Windows signing, Linux packages; `check-bundle.sh` and the release workflow; one release dry run per OS | implementer | 2 |
| B7 | UI: presets and custom picker with the estimator's numbers, consent and TCC messages, live phases, "Still working" / "Not responding", sustained chart, results with spread and last run, history columns, compare, export | implementer | 3 |
| B8 | Tests not covered above, `vm-smoke` plan, user guide, features, screenshots with independent review (ER-23), roadmap | implementer, reviewer | 1.5 |

Total about **25 agent-days** (re-estimated after review: the third-OS signing and notarizing of a new sidecar, direct
I/O and GPU on the Windows and Linux runners, the Linux unit handling). Order: B2's Linux spike and B1 first; B3 to B6
in parallel; B0 and B6a with the kinds they ship; B7 after B1/B2; B8 last.

## Decisions for the user (with recommendations)

Each recommendation below is the author's after the independent review; where the review changed or qualified it, the
line says "review".

- **D1 Priority.** Measure at normal priority as a named, narrow exception to rule 52's gentle default for bulk work,
  with the marker and conformance note of section 2, rather than the brief's background priority. *Recommended
  (review):* normal, recorded as the exception.
- **D2 GPU in its own runner** `dyno-bench-gpu`. *Recommended (review):* yes (crash isolation, size, build time); its
  signing and notarizing cost is WP-B6a.
- **D3 Disk defaults:** 1 GiB file, 3 repetitions, at most 14.5 GiB written per drive per run as counted by
  `disk_write_budget` (fill, ramps and measured parts), sustained write off unless confirmed. *Recommended (review):*
  yes; alternative 5 repetitions like CrystalDiskMark (at most 23.5 GiB).
- **D4 No composite score.** *Recommended (review):* none; per-metric numbers with spread only.
- **D5 Linux:** start the runners as a named transient unit through `systemd-run --user`, stopped with `systemctl --user
  stop` on Cancel, and cleaned up by invocation id; spike first. *Recommended (review):* yes, fallback marked low
  confidence.
- **D6 Battery:** consent per run; stop on unplugging without consent. *Recommended (review):* yes.
- **D7 Sustained length:** 5 minutes default (2, 10, 20 offered where a temperature is readable; 5 at most where none
  is). *Recommended (review):* 5, with the no-sensor limit.
- **D8 SIMD dispatch** through `multiversion` rather than hand-written `std::arch`. *Recommended (review):* `multiversion`
  (MIT OR Apache-2.0, accepted by the review); hand-written `#[target_feature]` functions with run-time detection are
  the smaller alternative if the dependency is unwanted.
- **D9 1.0 history:** keep and mark "measured the old way". *Recommended (review):* keep (cheap). The review notes
  that dropping the 1.0 history is simpler and probably loses nothing, since 1.0 never produced real runs in an install
  (finding 1).
- **D10 Shipping the runner.** WP-B0 lands in 1.1.0, bundled only together with the fixed disk test (B3) or with Disk
  turned off; the 1.0 disk test is never shipped. If a 1.0.x is cut, it only changes the missing-runner text from
  "reinstall Dyno" to "Benchmarks arrive in Dyno 1.1". *Recommended (review):* yes.
