# 0026. macOS sockets come from the kernel's socket list, never from a sweep over other programs' descriptors

- Status: Proposed (2026-10-31; the user decided the Dyno 1.0 tag waits for this). Needs an independent review before
  anything beyond the spike (`crates/dyno-platform/src/parse/mac_pcblist.rs`, `src/macos/pcblist.rs`) is built.
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

### What the kernel sends [M, S]

- A list is a 24-byte `xinpgen` header (`xig_len`, `xig_count`, `xig_gen`, `xig_sogen`), then per socket a run of
  records, then the same 24 bytes as a trailer. Every record starts `{ len: u32, kind: u32 }` and is padded to 8 bytes
  (`ROUNDUP64` in `in_pcblist.c` `get_pcblist_n`; `netstat` advances the same way, `inet.c:284`). Per socket the order is
  `xinpcb_n` (kind 0x10, 104 bytes), `xsocket_n` (0x01, 104), `xsockbuf_n` receive (0x02, 32) and send (0x04, 32),
  `xsockstat_n` (0x08, 136) and, for TCP only, `xtcpcb_n` (0x20, 204, padded to 208). Sockets of the userland network stack
  (Skywalk) are appended by `nstat_userland_list_snapshot` and may be other records; they are skipped by their length.
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
  | `xsocket_n` (>= 76 bytes read) | `xso_protocol` i32 | 36 |
  | | `so_last_pid`, `so_e_pid` i32 | 68, 72 |
  | `xtcpcb_n` (>= 40 bytes read) | `t_state` i32 (`TCPS_*`: 1 LISTEN .. 10 TIME_WAIT) | 36 |

### Measured against `netstat -anv` [M]

With a binary the kernel shows the whole list to (next section), the parser's TCP rows (244) and UDP rows (46) matched
`netstat -anv -p tcp` / `-p udp` captured at the same moment on (protocol, local port, remote port, state, pid): 0 rows
only on our side, 0 only in netstat, IPv4 and IPv6, listeners, TIME_WAIT, CLOSE_WAIT, FIN_WAIT, UDP bound and connected.
Reproduce: the ignored test `macos::pcblist::tests::the_list_agrees_with_netstat_anv`.

### Owner: `so_last_pid`, not the set of holders [S, M]

`netstat -anv` prints `so_last_pid` as `process:pid` (`netstat.tproj/main.c:867`, `in_pcblist.c:141`); `so_e_pid` is the
effective pid of a delegated socket. The kernel does not list every process that holds a descriptor. For a socket
shared by a parent and a child the row names the process that last used it, where the old sweep listed every holder and
Dyno then picked the parent (`socket_owner::owners`). This is cheaper and simpler; the change is that an inherited socket
is attributed to its last user. A connection in TIME_WAIT has no socket any more (the kernel sends zeros) and so no owner,
as `netstat` shows it. We show `so_last_pid` as the owner, and `so_e_pid` only when it differs (a delegated socket).

### The visibility rule, found while measuring [M]

On macOS 27.2 a process receives the whole list only when its **code-signing identifier contains a dot**; otherwise the
kernel sends only that process's own sockets (the header still announces them all: 160 announced, 1 sent). Same binary,
same user, same session; only `codesign -i <identifier>` differed: `a.b`, `x.`, `test.probe`,
`com.decryp7.dyno-agent` see everything; `probe`, `dyno-agent`, a linker-signed `cc` or Rust binary (identifier
`name-<hash>`) see only their own. Apple's binaries (netstat, sysctl), the python.org Python and a bundle executable
signed with its bundle id see everything. This is not in the XNU source (the public source has no such filter); it is
the behaviour of the shipped kernel and may differ on other macOS versions, which I could not test. The consequences:

1. **The shipped agent today would see nothing**: in `Dyno_1.0.0_aarch64.dmg` (handoff s22e) the bundle and `dyno-app` are
   `net.decryptology.dyno`, but `Contents/MacOS/dyno-agent` has the identifier `dyno-agent-<hash>` and `dyno` likewise.
   The release must sign both sidecars with a dotted identifier (`net.decryptology.dyno.agent`, `...dyno.cli`), and the
   bundle check must assert it (a release-checklist item and a CI test over the built bundle).
2. **The parser reports a cut-down list** (`PcbList::restricted()`: announced count over what was sent by more than
   max(16, a quarter)) and the adapter then returns no rows with the plain reason, below. Development builds and
   `cargo run` binaries (no dotted identifier) land here by design; their tests use their own sockets, which are always
   visible.
3. Do not trust the rule beyond what is measured: the detection is by the announced count, not by looking at the
   identifier, so a change in the rule degrades to the same visible message instead of to wrong data.

### What the page gains and loses

| | now (`proc_pidfdinfo` sweep) | kernel list |
|---|---|---|
| Effect on other programs | can set `SS_DRAINING` on theirs | none: no descriptor or I/O reference is taken; the kernel copies its tables out |
| Cost per sample | one `PROC_PIDLISTFDS` per process plus one `proc_pidfdinfo` per socket (N x M syscalls, hundreds of ms) | 2 sysctls: 0.7 ms (TCP, 244 sockets) and 0.13 ms (UDP, 46) in a debug build [M]; plus a brief exclusive hold of the protocol's pcb lock in the kernel [S: `lck_rw_lock_exclusive(&pcbinfo->ipi_lock)`], once per sample |
| Other users' sockets | pid unresolved without root, row has no pid | listed with their `so_last_pid`; the process name comes from Dyno's own process table, which already shows them as one "Other users" row |
| Owner of a shared socket | every holder, parent picked by a heuristic | the last user (one pid); `socket_owner` is not needed on macOS |
| Listeners, states, IPv4/IPv6, UDP | yes | yes (identical in the comparison) |
| Unix-domain sockets | not listed (netstat2 TCP/UDP only) | not listed (another sysctl, `net.local.*.pcblist64`; out of scope, as before) |
| Layout stability | libproc, public API | private structs; see the next section |

### Parse safety and the fallback

- The parser (`parse::mac_pcblist::parse`) is pure and compiled on every OS. It reads only through checked slices; a list
  over 64 MiB is refused unread; at most 262,144 sockets are kept; every record must lie inside the buffer; a record
  shorter than the bytes read from it is `UnknownLayout`, a record that runs past the end or has a length under 8 is
  `Malformed`; records of kinds it does not know are skipped by their own length (so a longer struct from a newer macOS
  still parses, and a new record kind does not stop it); sockets whose generation is newer than the header's (freed
  while copying) are left out as `netstat` does. A test truncates the fixtures at every length and changes every byte to
  four values: no panic.
- Reading: `sysctlbyname` with a buffer of the asked size plus a quarter; `ENOMEM` (the list grew) is retried up to 8
  times, then it is an unavailable reading.
- **Fallback if the layout is unknown or the view is cut down: the page shows the counts it can know and says why, and
  never falls back to the descriptor sweep.** Concretely the sockets port returns an unavailable reading with the reason
  ("macOS reported this program's own sockets only" or "macOS changed the socket list format"); the totals (the kernel's
  announced counts, and the user's own listeners if the view is restricted) are shown with that note. A fix is a new
  parser version, not a quieter sweep.

### No effect on other processes

The sysctl handler runs entirely in the kernel, copies its own tables to our buffer and takes no reference on any file
descriptor (`get_pcblist_n` never touches `fileproc`); nothing of ours is attached to another program's socket. This is
the property that removes the EBADF source. [S] The held pcb lock is shared with the network stack, for the copy of a
few hundred records, once per sample; if a measurement on a busy host shows a stall, the sample interval is the lever
(ER-20), not a retry.

## Consequences

- The Network page on macOS is exact for TCP and UDP and no longer able to harm other programs; the Dyno 1.0 tag waits
  for this to land.
- A new release gate: the built bundle's `dyno-agent` and `dyno` carry a dotted code-signing identifier, and a
  bundle-level smoke test (the packaged agent lists a socket another process owns) runs on the Mac runner before the tag.
- Risk: private struct layout and the visibility rule can change in a macOS update; both degrade to the explained
  message. The offsets are pinned by fixtures and by the ignored comparison test, run by hand on each new macOS.
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
