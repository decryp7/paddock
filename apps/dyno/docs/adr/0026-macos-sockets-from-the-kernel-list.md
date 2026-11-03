# 0026. macOS sockets come from the kernel's socket list, never from a sweep over other programs' descriptors

- Status: Accepted (2026-10-31, after independent review rounds 1 and 2 with no open P1; the user decided the Dyno 1.0 tag
  waits for this). Round 1: the choice stood, five P1 findings and the P2s are taken into the text below (marked "review").
  Round 2: the family-check correction and the layout alarm were found and fixed; the live comparison then had 0 skipped.
- Date: 2026-10-31
- Related: ADR 0015 (network page), ADR 0021 (monitoring only: Dyno must not change the machine, and a read that can
  break another app's connection is a change), ER-14, ER-20 / rule 51 (threads or cheaper), rule 53 (reusable OS names),
  `/Volumes/d7sst7/dev-targets/handoff/dyno-ebadf.md` (the root cause and the XNU references).
- Evidence marks: [M] measured on this Mac (macOS 27.2, build 26B123, Apple M-series, 2026-10-31); [S] read in source
  (XNU `main` head f6217f891ac0, network_cmds `main` head 97e27e6244c1, read 2026-10-31); [I] inference.

## Context

1. Today `OsSockets` on macOS is `netstat2`, which calls `proc_pidfdinfo(PROC_PIDFDSOCKETINFO)` on every socket
   descriptor of every process, at agent start, every 30 s and on the Network page. Each call holds an I/O reference on
   the other program's descriptor [S: `proc_info.c` `fp_get_ftype` .. `fp_drop`].
2. If that program (or a child it spawned, which holds a copy of every descriptor until its exec) closes the descriptor
   while we hold the reference, XNU drains it and sets `SS_DRAINING` on the shared **socket**, for good; every later
   read or write that must wait on it fails with EBADF [M: 12 of 100 loaded runs with the sweep, 0 of 100 without;
   S: `soo_drain`, `sbwait`]. So the sweep can break another app's connection. Dyno only watches (ADR 0021); this is a
   read with a side effect on others, which is not acceptable and cannot be made safe by retrying.
3. The same view exists without touching descriptors: the kernel's protocol control block lists, which is what
   `netstat -anv` prints (`process:pid`).

## Decision

On macOS the socket source is `sysctlbyname("net.inet.tcp.pcblist_n")` and `("net.inet.udp.pcblist_n")`, one call each,
parsed by a pure, bounded parser. The `proc_pidfdinfo` sweep is removed from macOS builds (the `netstat2` dependency stays
for Linux). Other OSes are unchanged. It sits behind the existing `NetworkSockets` port, so the agent and the page model
do not change.

**The same hazard in the two `lsof` uses is removed in the same change (review P1-4).** `lsof` calls
`proc_pidfdinfo` for every socket descriptor it meets, and for every pipe and kqueue unless asked for sockets only
(`dproc.c` `process_socket`, `process_pipe`, `process_kqueue`), whatever the selection. So `macos/details.rs`
(`lsof -p <pid>`) and `macos/locks.rs` (`lsof -- <path>`, which walks every process) had the same drain hazard as the
sweep, system-wide in the second case. Both move to `libproc` (`macos/libproc.rs`): descriptors are only *listed*
(`PROC_PIDLISTFDS` reads the table and holds nothing) and information is asked only for **vnode** descriptors. Vnodes are
safe: `vfs_vnops.c` has `.fo_drain = fo_no_drain`, where `soo_drain` (sockets), `pipe_drain` and `kqueue_drain` are the
descriptor types that act on a drain [S]. The details page's connections come from the kernel list filtered by owner
(`so_e_pid`/`so_last_pid`), its mapped libraries from the process's memory regions (`PROC_PIDREGIONPATHINFO`), and the
lockers of a file from the identity (device and inode, so a hard link is the same file) of every process's vnode
descriptors, bounded by the number of descriptors looked at, not by a clock (ADR 0031, ER-19). I did not measure the drain
on pipes or kqueues; vnodes-only avoids the question.

Why the vnode query is safe even when the descriptor changed between the listing and the question [S, review]: in
`kern_descrip.c`, `fp_get_ftype` checks the descriptor's type under `proc_fdlock` *before* it takes the I/O reference
(`os_ref_retain_locked` on `fp_iocount`), and returns `ENOTSUP` for another type without taking one. A number reused by a
socket in the meantime therefore costs nothing; at worst a query for a vnode briefly delays the target's `close()` (the
lock is held for a moment) and causes no error.

### What the kernel sends [M, S]

- A list is a 24-byte `xinpgen` header (`xig_len`, `xig_count`, `xig_gen`, `xig_sogen`), then per socket a run of
  records, then the same 24 bytes as a trailer. Every record starts `{ len: u32, kind: u32 }` and is padded to 8 bytes
  (`ROUNDUP64` in `in_pcblist.c` `get_pcblist_n`; `netstat` advances the same way, `inet.c:284`). Per socket the order is
  `xinpcb_n` (kind 0x10, 104 bytes), `xsocket_n` (0x01, 104), `xsockbuf_n` receive (0x02, 32) and send (0x04, 32),
  `xsockstat_n` (0x08, 136) and, for TCP only, `xtcpcb_n` (0x20, 204, padded to 208). Sockets of the userland network stack
  (Skywalk) are appended by `nstat_userland_list_snapshot` and may be other records; they are skipped by their length.
  A group is closed by the next `xinpcb_n` or by the trailer, not by one particular kind (review P1-5): a kernel that
  drops or renames a kind cannot make every group wait forever and leave an empty list without a message.
- The structs are declared in `bsd/netinet/in_pcb.h` (`struct xinpcb_n`, line 539 area), `bsd/sys/socketvar.h`
  (`struct xsocket_n`, `xsockbuf_n`, `xsockstat_n`, `XSO_*` kinds, lines 487-560) and `bsd/netinet/tcp_var.h`
  (`struct xtcpcb_n`, line 1610 on), all inside `#ifdef PRIVATE`, under `#pragma pack(4)`. **The macOS SDK on this Mac
  (MacOSX27.2.sdk, `usr/include`) ships only the public `struct xinpgen` (`netinet/in_pcb.h:254`) and `TCPT_NTIMERS_EXT 4`
  (`netinet/tcp_timer.h:70`); the `_n` structs are not in it.** They are not API and Apple may change them, so offsets
  are derived from the open XNU source and checked against the bytes of this Mac, not from a header we can include.
- Fields read, with byte offsets under pack(4) (verified on this Mac by exact agreement below):

  | record | field | offset |
  |---|---|---|
  | `xinpgen` | `xig_count` u32 / `xig_gen` u64 | 4 / 8 |
  | `xinpcb_n` (>= 80 bytes read) | `inp_fport`, `inp_lport` (network order) | 16, 18 |
  | | `inp_gencnt` u64, `inp_vflag` u8 (1 = IPv4, 2 = IPv6) | 28, 44 |
  | | foreign / local address (16 bytes; an IPv4 address is the last 4) | 48, 64 |
  | `xsocket_n` (>= 76 bytes read) | `xso_protocol`, `xso_family` i32 | 36, 40 |
  | | `so_last_pid`, `so_e_pid` i32 | 68, 72 |
  | `xtcpcb_n` (>= 40 bytes read) | `t_state` i32 (`TCPS_*`: 1 LISTEN .. 10 TIME_WAIT) | 36 |

### Measured against `netstat -anv` [M]

With a copy of the test binary re-signed with a dotted identifier (`codesign --force -s - -i net.decryptology.dyno.spike`,
next section) and `netstat -anv -p tcp` / `-p udp` captured from a shell just before, the parser read 174 TCP and 44 UDP
sockets, skipped none, and differed from netstat on (protocol, local port, remote port, state, pid) by 1 TCP row on
each side (a connection that opened or closed between the two captures) and 0 UDP rows. IPv4, IPv6, v4-mapped peers on
dual-stack sockets, sockets with an unset family, listeners, TIME_WAIT, CLOSE_WAIT, FIN_WAIT, UDP bound and connected.
Reproduce: the ignored test `macos::pcblist::tests::the_list_agrees_with_netstat_anv` (its own failure bar: under 5 %
differing). It compares the pid netstat prints (`so_last_pid`) with the socket's last user (`via_pid` for a delegated
socket), so delegated sockets, whose owner here is `so_e_pid`, do not count as differences. **Correction (review P1-1):** the first version of the parser required `xso_family` to agree with
`inp_vflag` and dropped 38 real sockets (27 dual-stack AF_INET6 sockets with an IPv4 flag, 11 with family 0); the
comparison had not been re-run after that check was added, and the fixtures were generated from the same rule, so they
could not catch it. The family is now only checked to be one that exists (0, AF_INET, AF_INET6), the address family is
read from `inp_vflag` as `netstat` does, and the fixtures carry both shapes written out by hand, not derived from the
parser's rule.

### Owner: `so_e_pid` when set, else `so_last_pid` [S, M]

`netstat -anv` prints `so_last_pid` as `process:pid` and, with `-vv`, `so_e_pid` (`netstat.tproj/main.c:867-889`;
`in_pcblist.c:141`). `so_last_pid` is the last process that used the socket; `so_e_pid` is the *effective* pid, set when a
socket works on behalf of another process (a delegated socket). Measured here: of 113 TCP sockets, 91 had `so_e_pid` 0
(unset), 21 had both equal, 1 differed (`com.apple.WebKit.Networking` working for `Safari`); of 45 UDP sockets 43 unset.
The page answers "which program is talking", and for the one differing socket that is Safari, not its helper. So the
row's owner is `so_e_pid` when set, else `so_last_pid`, and the other is kept as `via_pid` (the details page of either
process lists the connection). The kernel does not list every process that holds a descriptor: for a socket shared by a
parent and a child the row names the process that last used it, where the old sweep listed every holder and Dyno then
picked the parent by a heuristic (`socket_owner::owners`, no longer used on macOS). A connection in TIME_WAIT has no
socket any more (the kernel sends zeros) and so no owner, as `netstat` shows it.

### The visibility rule, found while measuring [M]

**Measured on macOS 27.2 (26B123) only.** There a process receives the whole list only when its **code-signing identifier
contains a dot**; otherwise the kernel sends only that process's own sockets (the header still announces more: 160
announced, 1 sent). Same binary, same user, same session; only `codesign -i <identifier>` differed: `a.b`, `x.`,
`test.probe`, `com.decryp7.dyno-agent` see everything; `probe`, `dyno-agent`, a linker-signed `cc` or Rust binary
(identifier `name-<hash>`) see only their own. Apple's binaries, the python.org Python and a bundle executable signed with
its bundle id see everything. **On the macOS 26.6.2 VM every identifier saw the full list (review P1-2)**, so this is a
27.x behaviour, and the review's observation is that it may depend on the *responsible process* (how the program was
launched), not on the identifier alone; I measured from shell children only. The public XNU source has no such filter.
Consequences:

1. **A shell run proves nothing about the shipped agent.** The release gate runs the *installed* bundle's agent under its
   LaunchAgent on a 27.x machine and checks that its Network answer lists a socket owned by a pid other than the
   agent's own (the page open in the installed app, or the `dyno` command), as a line of the release checklist
   (`docs/releasing.md`, and the VM runbook). The CI bundle check (below) is necessary, not sufficient.
2. **The shipped sidecars had no dotted identifier, and Tauri would reset one that was set** (review P1-3). In
   `Dyno_1.0.0_aarch64.dmg` the bundle and `dyno-app` are `net.decryptology.dyno`, but `Contents/MacOS/dyno-agent` and
   `dyno` have `dyno-agent-<hash>` and `dyno-<hash>`; Tauri signs every `externalBin` with `codesign --force -s -` and no
   `-i`, which replaces any identifier we set on the file. A Mach-O with an embedded `__TEXT,__info_plist` keeps its
   `CFBundleIdentifier` as the identifier through that re-sign (measured: a binary linked with
   `-Wl,-sectcreate,__TEXT,__info_plist,Info.plist` and re-signed with `codesign --force -s -` reports
   `Identifier=<CFBundleIdentifier>` and sees the whole list). So `dyno-agent` and `dyno` get an `Info.plist`
   (`net.decryptology.dyno.agent`, `net.decryptology.dyno.cli`) embedded by their `build.rs`
   (`cargo:rustc-link-arg-bins`). Gates: a test per binary copies it, re-signs it the way Tauri does and fails unless
   the identifier matches `^net\.decryptology\.dyno\.`; `scripts/check-bundle.sh` runs `codesign -dv` on the built
   bundle's `Contents/MacOS/dyno-agent` and `dyno` and fails on any other identifier.
3. **A cut-down list is detected, not guessed** (review P1-1). The header's `xig_count` includes dead control blocks
   (measured: 245 announced, 45 sent after UDP churn, and 34/14 on the VM), so the count alone gives false alarms, and on a
   quiet host a gap under 16 never fired. The rule is: `announced > sent` AND no socket in the list belongs to a process
   other than Dyno's own pid (`PcbList::restricted(own_pid)`, the pid passed in so the parser stays pure). A host whose
   only live sockets are Dyno's own cannot be told from a cut-down view and is reported as cut down (the safe side).
   The opposite error is accepted and documented: a cut-down view that contains a socket whose `so_last_pid` is a child of
   Dyno (an inherited socket another process used last) is not recognised and is shown as if whole. Development builds,
   `cargo run` binaries and unit tests (identifier without a dot) land here by design; unit tests
   look at their own sockets, which are always in the list.
4. The detection works from the list itself, so a change in the rule on another macOS version degrades to the same
   visible message instead of to wrong data.

### Amendment 2026-11-03: the dotted identifier is necessary, not sufficient [M]

The released `Dyno_1.0.0` bundle has the right identifiers (`dyno-agent` = `net.decryptology.dyno.agent`, `dyno` =
`net.decryptology.dyno.cli`, checked with `codesign -dv` on the published `Dyno.app.tar.gz`), and the Network page still said
"only its own sockets" (app log 2026-11-01 14:20:51, request `Network`, answered by an agent whose parent was the app, not
launchd). Probes on macOS 27.2 (26B123), same dotted ad-hoc binary: from a shell, as a `launchctl submit` job and as a
`launchctl bootstrap`ed LaunchAgent it sees the whole list (113 to 119 of 121 announced); as a **child of an ad-hoc signed
`.app` started by LaunchServices** (directly or through `/bin/sh`) it sees 0 of 113. `responsibility_spawnattrs_setdisclaim` did
not change that. So the app's own fallback launch (`ProcessLauncher`, a plain child) gets the cut-down view; the login service
does not. Follow-ups: the page text names the real identifier and the way out; the agent logs `Agent socket view` at start;
`check-bundle.sh` runs `dyno-agent view` as a launchd job. **Open design question:** start the agent from the app through launchd
instead of as a child.

### What the page gains and loses

| | now (`proc_pidfdinfo` sweep) | kernel list |
|---|---|---|
| Effect on other programs | can set `SS_DRAINING` on theirs | none: no descriptor or I/O reference is taken; the kernel copies its tables out |
| Cost per sample | one `PROC_PIDLISTFDS` per process plus one `proc_pidfdinfo` per socket (N x M syscalls, hundreds of ms) | 2 sysctls: 0.7 ms (TCP, 244 sockets) and 0.13 ms (UDP, 46) in a debug build [M]; plus the exclusive pcb lock, see "The lock" below |
| Other users' sockets | pid unresolved without root, row has no pid | listed with their `so_last_pid`; the process name comes from Dyno's own process table, which already shows them as one "Other users" row |
| Owner of a shared socket | every holder, parent picked by a heuristic | the last user (one pid); `socket_owner` is not needed on macOS |
| Listeners, states, IPv4/IPv6, UDP | yes | yes (identical in the comparison) |
| Unix-domain sockets | not listed (netstat2 TCP/UDP only) | not listed (another sysctl, `net.local.*.pcblist64`; out of scope, as before) |
| Layout stability | libproc, public API | private structs; see the next section |
| Details page (files, connections, libraries) | `lsof -p` (same hazard) | `libproc` vnode descriptors + the kernel list (no socket, pipe or kqueue is asked about) |
| "What is locking this file" | `lsof -- path` over every process (same hazard, system-wide) | `libproc` vnode descriptors only |

### Parse safety and the fallback

- The parser (`parse::mac_pcblist::parse`) is pure and compiled on every OS. It reads only through checked slices; a list
  over 64 MiB is refused unread; at most 262,144 sockets are kept; every record must lie inside the buffer; a record
  shorter than the bytes read from it is `UnknownLayout`, a record that runs past the end or has a length under 8 is
  `Malformed`; the socket record's `xso_family` must be one that exists (0, AF_INET or AF_INET6; not required to agree with
  the control block's flags, which real sockets do not always do); a record may not run into the 24-byte trailer; when
  groups were sent and most of them could not be read because records were missing or held impossible values, the answer
  is `UnknownLayout`, not an empty list. That judgement is made on the groups actually seen, never on `xig_count`, which
  counts dead control blocks (a fixture with the socket records removed and 40 announced still raises it), and it is made
  per protocol before the two lists are merged, so one protocol that drifted fails the whole read instead of silently losing
  its rows (review P1-2, P1-5); records of kinds it does not know are skipped by their own length (so a longer struct from a newer macOS
  still parses, and a new record kind does not stop it); sockets whose generation is newer than the header's (freed
  while copying) are left out as `netstat` does. A test truncates the fixtures at every length and changes every byte to
  four values: no panic.
- Reading: `sysctlbyname` with a buffer of the asked size plus a quarter; `ENOMEM` (the list grew) is retried up to 8
  times, then it is an unavailable reading. One buffer is kept between samples and its pages are written before the call
  (review P2), see "The lock".
- **Fallback if the layout is unknown or the view is cut down: the page shows the counts it can know and says why, and
  never falls back to the descriptor sweep.** Concretely the sockets port returns an error with the reason ("macOS showed this program only its own
  sockets. The agent needs a code-signing identifier with a dot ..." or "This version of macOS changed the format of its
  socket list ..."), which the Network page shows in its error state with Retry. **Totals-only is not built**: the page
  and the protocol have no field for "counts without rows", and adding one is a follow-up (the announced counts are
  already in the parsed list). A fix is a new parser version, not a quieter sweep.

### No effect on other processes

The sysctl handler runs entirely in the kernel, copies its own tables to our buffer and takes no reference on any file
descriptor (`get_pcblist_n` never touches `fileproc`); nothing of ours is attached to another program's socket. This is
the property that removes the EBADF source. [S]

### The lock (review P2)

`get_pcblist_n` takes `lck_rw_lock_exclusive(&pcbinfo->ipi_lock)` and keeps it **while it copies every record out to our
buffer** (`SYSCTL_OUT` inside the loop, `in_pcblist.c:402-440`), not only while it builds a list. While it is held, the
protocol's pcb lookups wait: new connections, closes and lookups of that protocol stall for the copy. Mitigations:
the buffer is reused and its pages are written before the call, so the copy does not take page faults under the lock;
the copy is a few hundred records (about 1 ms in a debug build here, 244 TCP rows); and the sample cadence bounds it.
The worst case is the Network page: the agent caches a read for 1.5 s (`dyno-agent/src/network.rs`), so a page polling
every second causes at most one pair of reads per 1.5 s, plus the 30 s watcher. If a measurement on a busy host shows a
stall, the interval is the lever (ER-20); a retry is not.

## Consequences

- The Network page on macOS is exact for TCP and UDP and no longer able to harm other programs; the Dyno 1.0 tag waits
  for this to land.
- A new release gate: the built bundle's `dyno-agent` and `dyno` carry a dotted code-signing identifier, and a
  check of the installed app (its Network page lists a socket another program owns) is the manual line in
  `docs/releasing.md` before the tag; CI cannot run the installed LaunchAgent.
- Risk: private struct layout and the visibility rule can change in a macOS update; both degrade to the explained
  message. The offsets are pinned by fixtures and by the ignored comparison test, run by hand on each new macOS.
- `lsof` leaves the macOS adapters entirely; the Linux adapters keep it (it reads `/proc` there, no descriptor of a
  socket is asked about).
- Not done here and decided separately: the IPC no-wait fix and the Platform-owned ports in agent tests
  (handoff fixes 1 and 2); they matter for the tests, not for this ADR.

## Alternatives considered

- **Keep `proc_pidfdinfo`, make it safer.** There is no way to take the reference without the drain race. Rejected.
- **`lsof -i` / `netstat -anv` as a child process.** Safe for others (netstat uses this sysctl), but a process spawn per
  sample, text parsing, and, measured here, a `netstat` started from a test process printed nothing. Rejected as the
  main path; the parsed `netstat` output stays the reference in the comparison test.
- **`nettop`/NetworkStatistics.framework.** Private framework, 5 s per read (the existing traffic counters pay that
  already); it has no listeners-without-traffic view. Rejected for this page.
- **`net.inet.tcp.pcblist64` (older, public-ish `xtcpcb64`).** Exists, in the SDK (`struct xinpcb64`, `xsocket64`), but has
  no `so_last_pid`/`so_e_pid`, so no owner. Rejected for the owner column.
