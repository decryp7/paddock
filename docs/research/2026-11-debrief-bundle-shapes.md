# Debrief and a large industrial station diagnostic bundle: which file shapes it reads

Date: 2026-11-05. Input for Debrief 1.2's roadmap. The bundle is confidential: this note holds only generic family
descriptions, counts and invented shapes. Every sample was invented; no real file was read.

**Refs.** Measured at `origin/debrief/p0-idle` 7b0c2421e through `import_path`, the app's import path, with every built-in
plugin over real IPC and the built-in parser library; `caps` ran each file in a plugin process with the default cap, and
`folder` used `DuckStore`. `git diff` shows `importers/`, `ingest/`, `parser/`, `parsers/`, `time.rs`, `text.rs`, `serve.rs`
and the format plugins identical between that ref and `origin/debrief/1.2-int` 852b35ac0, so these results carry to 1.2-int
(not re-run). `origin/main` 87c9b8449 differs on those paths only by a capture refactor in `lines.rs` and `text.rs`.
`shapes` and `families` were also run at `origin/main`: outputs identical. `caps`, `quarantine` and `folder` were not re-run there (from code; the plugin
paths are unchanged, and main is on DuckDB 1.5.6, so only `folder` could differ).

Paths are below `apps/debrief/crates/`. **Basis:** `m` measured on an invented same-shape file, `c` from code. Case names
are from the scratch probe. Verdicts for real files are by kind, encoding and shape from the inventory: their content
beyond the first unstamped line was not available, so where that decides the outcome, it says so. Timings are from one
loaded machine, in-memory sink unless stated, and are not a benchmark (rule 48: add a results block before quoting them).

## Answer

About a sixth of the bytes get the right time today. Most of the rest are non-log project data, but the largest and most
useful file kinds also fail: all three EVTX and the registry export hit the plugin memory cap. Three such failures
**quarantine the plugin**. Two shapes import quietly with a wrong time.

| Verdict | Files | Bytes | Share of bytes |
| --- | ---: | ---: | ---: |
| correct time | 1,972 | 1,402,512,666 | 17.2 % |
| no time | 44 | 52,186 | 0.0 % |
| wrong time: local read as UTC | 60 | 106,573,738 | 1.3 % |
| wrong time or misread | 717 | 348,699,014 | 4.3 % |
| refused or fails | 11,458 | 6,313,336,987 | 77.3 % |
| skipped by design | 4 | 144,752 | 0.0 % |
| **Total** | **14,255** | **8,171,319,343** | 100 % |

Refused or fails, by kind: proprietary binary 6,833 files, 1,083 MiB; XML 1,327, 1,384 MiB; text 2,656, 826 MiB; images 587,
186 MiB; EVTX, ZIP, `registry.pol` 9, 2,542 MiB; empty 46.

"Local read as UTC" means the digits are right but the instant is off by the station's offset against any log that has one.
The two event text exports in "misread" are the least certain: they become an untimed CSV only if the first 20 lines have
one `;` count (`csv.rs:402-421`), otherwise they are refused, moving 328 MB to "refused". Family 3 was run with padding, not
with real full-memory dumps.

## Whole folder

| Question | Verdict | Evidence |
| --- | --- | --- |
| File count limit | None hit. 14,255 tiny files in the bundle's mix: import 5.3 s, catalogue 22.8 s, summary 63 ms | `folder`, m. Real bytes not run. No file-count constant in the import path, c |
| Skipped by design | Only `.exe`, `.dll`, dot files and Debrief's own: 4 DLLs here. Everything else is read and judged | `default_profile.toml:22-23`, `discover.rs:96-105` |
| Stated reasons | Every refusal is listed with a reason: `binary file` 7,420, no parser 4,007, `empty file` 45 in the mix | `folder`, m; `ingest/source.rs:212-228`. Reasons are per file: no totals by reason, and the notice names 5 files (`workspace_commands.rs:128-132`, `app/src-tauri`) |
| Misleading reason | After a quarantine, a dump or a small EVTX reads `binary file` | `quarantine`, m |
| Size limit | Plugin cap 1,024 MB by default, 4,096 at most (`performance/mod.rs:82`, `section.rs:16`, `app/src-tauri`). A plugin file is also limited to 2 GiB (`debrief-plugin-api/src/serve.rs:25`) | EVTX passes at 64 MB (692 MB peak), stopped at 128 and 550 MB. 460 MB UTF-16 `.reg` stopped. Reported as "crashed", `caps`, m |
| Quarantine | Three over-cap EVTX, then a dump and a 4 MB EVTX: both refused. Only an order saves dumps: capitals sort first, so dump folders here come before lower-case root files, if nothing overlaps (c) | `quarantine`, m; `debrief-core/src/guard.rs:39` |
| EVTX with its text export | No duplicate today, as EVTX fails. Once it imports, both appear: nothing compares content | `discover.rs:211` dedupes paths; `fingerprint.rs:1-2` is rename detection only, c |
| `All` and `Summary` series | Both import, counted twice if one is a subset. `Summary` is 148 MiB of 418 MiB; the subset question is unmeasured | c, plus inventory |
| Sources in the Explorer | About 2,793 imported files in 141 rows, 12 of them groups, the largest 1,211 files. 11,458 refused files are each a row | `rolling.rs:33` regexes run over the file names, counts only; `shell/sidebar.ts:78,125`, `app/src` |
| Series that do not collapse | `name1..40.log` stays 40 rows; `~name.txt` is apart from `name.txt`; 2-digit sequence plus date stays apart | `families`, m |
| See, filter, undo | See: yes, rows are not virtualized, and the summary carries 4.4 MB of JSON each time (1.56 MB sources, 2.88 MB refused). Filter: `Query::sources` or `@source`. Remove: one source per click, 61 ms in the store, no bulk, no undo; refused rows have no remove | `folder`, m; `debrief-store-duck/src/lib.rs:1219`, `shell/sidebar.ts:172,359`, c |
| Handles and cost | One file open per reader, closed after each; readers at most 8. Serial cost 22.3 ms per small imported file, 1.0 binary, 1.6 XML | `ingest/source.rs:93`, `parallel.rs:30-35`, c; `shapes`, m |
| Skip proprietary files and name why | Partly: refused and named with a reason, at low cost as only the head is read. No skip list, no folder or family summary | m, plus the rows above |

## Families

The number is the family's position in the private inventory, so the owner can map it back.

| # | Family | Files | Bytes | Verdict | Basis | Evidence |
| ---: | --- | ---: | ---: | --- | --- | --- |
| 1 | Windows event logs, binary EVTX | 3 | 2,663,591,936 | refused or fails | m | Plugin stopped at its 1,024 MB cap, reported as a crash: 128 and 550 MB logs fail, 64 MB passes at 692 MB peak (caps) |
| 2 | Compiled project windows: binary, XAML, UTF-16-looking data, images | 7,193 | 2,375,297,475 | refused or fails | m | Refused: `binary file` (st15a, databind_binary) or no parser (xaml_bom) |
| 3 | Full crash dumps, about 296 MiB each | 3 | 932,629,763 | correct time | m | Header time; 296 MiB padded dump passes, 303 MB peak (caps). Real full-memory dumps not run |
| 4 | Event log text exports, `ctime;0xId;level;source;message` | 3 | 485,157,956 | wrong time or misread 2; refused or fails 1 | m | `ctime` is not read without `asctime` (time.rs:329); multi-line start refused; single-line start gives CSV, no time, first row eaten as header (evt_ctime_*) |
| 5 | Registry export, UTF-16, 439 MiB | 1 | 460,690,618 | refused or fails | m | Plugin stopped at the cap, reported as a crash (caps, 460 MB UTF-16). Small export imports untimed (st10) |
| 6 | Installer verbose logs, UTF-16, time of day only | 2 | 185,310,314 | refused or fails | m | Refused (ts11a); a user regex still gives no time, `time_only` comes only from templates (profile.rs:475) |
| 7 | Project databases, mostly binary | 1,468 | 161,807,834 | refused or fails 1,466; no time 2 | m | Refused (st15a); 2 `key,value` text files become untimed CSV (key_comma_lines) |
| 8 | Application log A, `date,time+offset` CSV | 193 | 146,430,371 | correct time | m | log4net-csv parser, right UTC, across DST (app_csv_quoted, app_csv_dst_cross); `__` stays literal |
| 9 | Application log B, same format | 95 | 146,025,520 | correct time | m | As family 8 |
| 10 | Application log C, same format | 322 | 145,757,290 | correct time | m | As family 8 |
| 11 | Runtime resources and caches, binary | 22 | 91,188,172 | refused or fails | m | `binary file` (runtime_cache_utf16_like) |
| 12 | Operation history, no offset, 8-bit, state-dump blocks | 41 | 81,571,594 | wrong time: local read as UTC 40; refused or fails 1 | m | Read, blocks kept in the event, local clock read as UTC (ope_cp1252_dump_blocks); current file's 329-line header is past the 200-line sniff, refused (ope_329_line_state_header) |
| 13 | Test data: images, small text, tool log with `Day=` stamps | 21 | 71,459,845 | refused or fails 19; skipped by design 2 | m | Refused (image_bmp, day_hour_tool_log); 2 DLLs excluded by glob |
| 14 | Screen captures, BMP | 4 | 66,355,416 | refused or fails | m | `binary file` (image_bmp) |
| 15 | Access-control definitions: binary, encrypted-looking, XML, ZIP | 369 | 23,496,024 | refused or fails | m | `binary file` (encrypted_8bit_no_nul); zip members refused one by one (zip) |
| 16 | Log-server lifecycle logs, same format as 8, 1,340 small files | 1,340 | 22,582,673 | correct time | m | As family 8; each file its own source, grouped in the Explorer |
| 17 | Agent logs, `[yyyy-MM-dd HH:mm:ss,ffffff]`, no offset | 8 | 21,966,800 | wrong time: local read as UTC | m | Read, exception lines attached, local clock read as UTC (bracket_micros_exception) |
| 18 | Security tool logs, month-first dates | 12 | 20,716,788 | refused or fails | m | Refused, generic regex wants the year first (ts09a, month_first_frames); a user parser with `day_first=false` reads them |
| 19 | Message history, `date time +offset,` CSV, 8-bit | 11 | 10,343,209 | wrong time or misread | m | Silent: regex backtracks, offset lands in the message, local clock read as UTC (hist_csv_offset_cp1252) |
| 20 | Alarm raw data, binary | 3 | 9,540,496 | refused or fails | m | `binary file` (st15a) |
| 21 | Engineering history, same shape as 19 | 575 | 9,379,710 | wrong time or misread | m | As family 19 (hist_csv_offset) |
| 22 | Small crash dumps, 19 plus 2 of 0 bytes | 21 | 9,087,049 | correct time 19; refused or fails 2 | m | Header time used, not file time (dmp1); 0 bytes `empty file` (st14a) |
| 23 | Revision records, text, CSV-like | 10 | 7,219,772 | refused or fails 9; wrong time: local read as UTC 1 | m | Comma counts vary, refused (rev_csv_date_first_row); 1 stamped file, local clock read as UTC |
| 24 | Trend buffers, binary | 2 | 5,984,256 | refused or fails | m | `binary file` (st15a) |
| 25 | UI configuration: XML, binary, UTF-16-looking data | 246 | 2,872,816 | refused or fails | m | Refused (config_xml, databind_binary) |
| 26 | System information text report, UTF-16 | 1 | 2,720,940 | refused or fails | m | Refused (st09); `[Section]` plus tabs is not CSV; only the XML `.nfo` has an importer |
| 27 | Setup logs, three stamp shapes, banners | 8 | 2,104,925 | wrong time: local read as UTC 4; wrong time or misread 2; refused or fails 2 | m | ISO and slash: local read as UTC (iso_install_log, ts04); `UTC=+h:mm` ignored, wrong by the offset (dot_date_utc_field_bom); 2 unstamped refused |
| 28 | Device-install log | 1 | 2,067,692 | refused or fails | m | Refused: stamps are not at line start (device_install_log) |
| 29 | Driver memory hex dumps as text | 3 | 1,307,788 | refused or fails | m | Refused (hex_dump_text) |
| 30 | Archiver readme and language files | 81 | 1,249,858 | refused or fails | m | Refused (seven_zip_lang) |
| 31 | Resource history, block header then rows | 2 | 996,848 | refused or fails | m | Comma counts vary, refused (resource_blocks) |
| 32 | Alarm-server log, `date time.fff +offset,` CSV, 8-bit | 127 | 655,447 | wrong time or misread | m | As family 19 (srv_csv_fraction_offset_cp1252) |
| 33 | OPC-style server log, fixed size, no offset | 2 | 655,360 | wrong time: local read as UTC | m | Read, local clock read as UTC (ts04); the `~` previous generation is a separate row |
| 34 | Per-window state, INI-like, 1,111 small files | 1,111 | 407,127 | refused or fails | m | Refused (per-window INI case) |
| 35 | Window IPC trace: UTF-16 one-liners, binary, empty | 500 | 349,303 | refused or fails | m | Refused (wm_trace_*) |
| 36 | Alarm configuration and status | 13 | 332,254 | refused or fails 10; no time 3 | m | Refused; 3 CSV become untimed (alarm_attrs_csv) |
| 37 | Circular log, colon before milliseconds | 1 | 327,680 | refused or fails | m | Refused (ts05, colon_millis_circular); with a user parser, line order is not time order and nothing marks it (st07) |
| 38 | Client and server configuration, 2 logs | 23 | 323,387 | wrong time: local read as UTC 2; refused or fails 21 | m | Logs: local read as UTC; the rest refused (config_xml, st15e) |
| 39 | WMI lists: `Key=Value`, CSV after a banner | 4 | 288,123 | refused or fails | m | Refused (wmi_key_value_utf16, wsh_banner_csv) |
| 40 | Alarm attributes: CSV, XML, JS | 40 | 211,099 | refused or fails 26; no time 14 | m | Header CSV untimed (alarm_attrs_csv); comment-first and others refused (js_file) |
| 41 | Network logs, `ctime`, separators | 3 | 158,962 | refused or fails | m | Refused (ctime_separators) |
| 42 | Builder resources: images, XML, text | 39 | 95,572 | refused or fails | m | Refused (image_jpeg, config_xml) |
| 43 | Printing logs and status | 4 | 87,095 | wrong time: local read as UTC 2; refused or fails 2 | m | Stamped: local read as UTC (ts04); listings refused |
| 44 | DirectX text report | 1 | 85,627 | refused or fails | m | Refused (dxdiag_report) |
| 45 | License files | 35 | 84,808 | refused or fails 18; no time 17 | m | `@key,value` become untimed CSV, first row eaten (key_comma_lines); the rest refused |
| 46 | Security snapshots | 6 | 61,353 | refused or fails | m | Refused (firewall_state, inf_utf16) |
| 47 | Scheduler log and data | 2 | 58,015 | wrong time: local read as UTC 1; refused or fails 1 | m | Log: local read as UTC (ts04); data refused (scheduler_data) |
| 48 | Group Policy files | 6 | 41,277 | refused or fails 5; no time 1 | m | `registry.pol` and XML refused (reg_pol); audit CSV untimed |
| 49 | Network diagnostic snapshots | 13 | 30,434 | refused or fails | m | Refused (firewall_state, ctime_add_ok) |
| 50 | Agent configuration, XML | 24 | 29,287 | refused or fails | m | Refused (config_xml) |
| 51 | `ipconfig` text, 8-bit | 1 | 25,157 | refused or fails | m | Refused (ipconfig_cp1252) |
| 52 | Process snapshots | 3 | 24,107 | refused or fails 2; no time 1 | m | CSV untimed (snapshot_process_csv); table and colon forms refused |
| 53 | Resource snapshot, block format | 2 | 22,845 | refused or fails | m | As family 31 |
| 54 | Per-user state, binary | 104 | 22,310 | refused or fails | m | `binary file` (st15a) |
| 55 | Product files: empty, text, DLL | 9 | 10,462 | skipped by design 2; refused or fails 7 | m | Empty and text refused; 2 DLLs excluded by glob |
| 56 | OPC registration: `.reg`, INI | 12 | 6,967 | no time 6; refused or fails 6 | c | REGEDIT4 `.reg` taken by the registry importer (debrief-plugin-registry/src/lib.rs:52), untimed; others refused |
| 57 | Vendor binary | 1 | 6,124 | refused or fails | m | `binary file` (st15a) |
| 58 | One-line package records | 40 | 2,103 | refused or fails | m | Fewer than 2 lines: no delimiter, refused (rev_record_one_line) |
| 59 | Driver setup history, `ctime` | 2 | 1,840 | refused or fails | m | Refused (ctime_add_ok) |
| 60 | License key codes | 21 | 1,664 | refused or fails | m | Refused (key_code_one_line) |
| 61 | Logger configuration, XML | 3 | 978 | refused or fails | m | Refused (config_xml) |
| 62 | One-line tool log | 1 | 408 | refused or fails | m | Refused (rev_record_one_line) |
| 63 | Script check output | 1 | 237 | refused or fails | m | Refused (wsh_banner_csv) |
| 64 | 8-byte placeholder files and empty files | 37 | 183 | refused or fails | m | Refused (dummy_tmp, st14a) |

## Shapes

| Timestamp at line start | Verdict | What happens | Citation |
| --- | --- | --- | --- |
| `yyyy/MM/dd,HH:mm:ss.fff+zz:zz` | Handled | Right UTC, across DST, via `log4net-csv` or the text regex | `parsers/log4net-csv.toml`, `generic.toml:64`; `importers/text.rs:345`; ts01, app_csv_dst_cross |
| `yyyy/MM/dd HH:mm:ss +zz:zz`, `.fff +zz:zz` | **Partly: wrong when a comma follows** | Followed by a space: right. Followed by `,`: the regex backtracks, the offset goes in the message, the clock is read as UTC, with no problem reported | `generic.toml:64` needs whitespace after `,?`; ts02, ts03, hist_csv_offset |
| `yyyy/MM/dd HH:mm:ss`, `yyyy-MM-dd HH:mm:ss`, `[yyyy-MM-dd HH:mm:ss,ffffff]` | Partly | Imported, read as UTC | `generic.toml:64,77`; ts04, ts07, ts08 |
| `yyyy/MM/dd HH:mm:ss:fff` | Not | Refused; a user parser reads it | `generic.toml:64` allows `[.,]` only; ts05 |
| `yyyy.MM.dd HH:mm:ss.fff UTC=+h:mm` | **Wrong** | Imported, `UTC=+h:mm` left in the message, read as UTC. A regex cannot join two captures into one timestamp | ts06; user parser t-utcfield |
| `MM/dd/yyyy HH:mm:ss` | Not | Refused; `day_first=false` in a user parser reads it, even when ambiguous | `generic.toml:64`; ts09a, ts09b |
| `Ddd Mon d HH:mm:ss yyyy` | Not | Refused; `asctime` must be named in a parser | `debrief-plugin-api/src/time.rs:329`; ts10 |
| `MSI (s) (xx:yy) [HH:mm:ss:fff]` | Not | Refused. A user regex imports it untimed: only output templates can be time-only, and those take the date from the file time, here the copy time | `debrief-core/src/profile.rs:475`, `importers/lines.rs:183`; ts11a |
| `Date,yyyy.MM.dd,HH:mm:ss` block | Partly or not | With consistent commas: CSV, 5 of 105 rows timed, rows do not inherit. With the bundle's mixed first lines: refused | ts12, resource_blocks |
| `[ Day=18 Hour=9 Min=57 Sec=32 ]` | Not | Refused; a user format gives no time | ts13, t-dayhms |
| Zone | Partly | `assume_timezone` is per parser and fixed (`"+09:00"` works, m) or the analyst's own; `Adjust clock` is per source. No bundle zone, no DST for local logs | `generic.toml:77`, `time.rs:405-411`, `shell/sidebar.ts:358` |

| Encoding | Verdict | What happens | Citation |
| --- | --- | --- | --- |
| UTF-16LE with and without BOM, UTF-8 BOM | Handled | Right text, also non-Latin without BOM | `ingest/encoding.rs:203-225`; enc1, enc2, enc3, enc7 |
| CP1252 | Partly | Dense: right. Under about 0.5 % invalid in the first 64 KiB: stays UTF-8, bytes become U+FFFD | `encoding.rs:17,231-235`; enc4a, enc4b |
| CP932 in ASCII | Partly | As CP1252; after 64 KiB of ASCII: U+FFFD, no `Encoding` problem reported | enc5a, enc5b, enc5c |
| ASCII with stray bytes | Handled | Kept with U+FFFD; many control bytes refuse the file as binary | `debrief-plugin-api/src/text.rs:28-35`; enc6a, enc6b |
| 8-bit in CSV | Not at this ref | Detected, then decoded lossily as UTF-8: every accent lost. `debrief/stream-text` reads CSV through the line reader with the detected encoding (c, not run) | `ingest/source.rs:356-359`; st13a, st13b |

| Structure | Verdict | What happens | Citation |
| --- | --- | --- | --- |
| `__` for a newline in a quoted CSV field | Partly | One event, `__` stays literal, no exception recognised | st01, app_csv_quoted |
| Multi-line records, `.NET` frames, state dumps | Handled | Attached to the event above; frames become the exception; date-like lines add an `UnmatchedLine` problem | `importers/lines.rs:7-15`; st02, st03, st04 |
| 329-line header | Not | Sniff looks at the first 200 non-blank lines: refused. 150 lines: fine, the header is one untimed event. With commas: all untimed CSV, stamps lost | `importers/text.rs:15`, `lines.rs:38-52`; st05a, st05b, st05c |
| Banners, separators | Handled | One untimed event at the start, otherwise attached | st06 |
| Circular log with marker | Partly | With a stamp it reads: file order kept, marker and fragment attached to the event above, nothing flags the wrap | st07 |
| Rolled series | Partly | See "Series" above | `rolling.rs` |
| `date;0xId;level;source;message` | Handled for readable dates | ISO-like or US AM/PM, and semicolons in the message, fine. `ctime`: see family 4 | st08a, st08b, st08c |
| msinfo32 text, `key=value`, CSV after a banner | Not | Refused; only XML `.nfo` has an importer | st09, st11a, st11b, st12a, st12b |
| `.reg` | Small: untimed. 460 MB: fails | Plugin stopped at its cap | st10, `caps` |
| 0 bytes, 8-byte placeholder | Refused | `empty file`, `binary file` or no parser; a signature-only dump: `truncated minidump` | st14a-d |
| Dump time | Handled | Header time, not the file's | `debrief-plugin-windows/src/minidump_import.rs:78`; dmp1 |
| Project XML, INI, numeric CSV | XML, INI refused; CSV untimed | Any 2+ lines with one delimiter count become untimed CSV, and their first row may be eaten | `csv.rs:88-97,402-421`; st15a-g, key_comma_lines |

## Gaps, ranked by bytes and diagnostic value

Sizes are rough, in days, before an ADR. "Y13" is 4 days of format gaps from the roadmap (`docs/roadmap.md:35-43`), `.tar`, journald
and others: **none of these except a first-row-is-data CSV, which already works.** It does not cover this bundle.

| # | Gap | Bytes | Plan | Size |
| ---: | --- | ---: | --- | ---: |
| 1 | EVTX fails above 64 to 128 MB | 2.48 GiB | PH4, 2.6b, 3.4. Add a 1.5 GiB case to PH4's 256 MB test | planned, 10 |
| 2 | A cap stop counts as a crash; 3 quarantine the plugin; the next files read `binary file`. Two ignored tests already name part of this | every plugin file | missing | 1 |
| 3 | `+offset,` silently loses the offset | 713 files, 19 MiB | missing | 0.5 |
| 4 | `ctime;...` exports, with continuation; `asctime` always on | 463 MiB | missing; streaming 3.5 for memory | 1.5 |
| 5 | `__` as newline, exception recognised | 1,950 files, 439 MiB | missing | 1 |
| 6 | Bundle zone with DST for local logs; `UTC=` field | 102 MiB plus future | missing | 3 |
| 7 | Refused files: totals by reason and folder, a skip list, bulk remove and undo, no 4.4 MB summary | 11,458 rows | M3b and 2.7 virtualize; 4.2 bounds the summary; the rest missing | 3 |
| 8 | EVTX and text export, `All` and `Summary`: choose or dedupe. Measure the subset first | 611 MiB once item 4 lands | missing | 3 |
| 9 | Registry export under the cap | 439 MiB | S2 in the streaming note, 3.4 | in S2 |
| 10 | Installer log: date from the header, time-only regex, not the file time | 177 MiB | missing | 1 |
| 11 | Month-first, colon milliseconds, block header, `Day=` | 21 MiB | missing | 1.5 |
| 12 | Snapshots with no line time as documents stamped at capture | about 13 MiB | missing | 2 |
| 13 | Sniff past a long header; wrap flag on circular logs; series names; encoding beyond 64 KiB | under 200 MiB, mostly already readable | missing | 2 |

**Streaming overlap.** `debrief/stream-text` already moves CSV off the whole-file path, which fixes 8-bit CSV and the memory of a
large CSV. It does not touch time parsing, delimiter choice or any row above except part of 4. The parser-streaming note
(`docs/research/2026-11-debrief-parser-streaming.md`, branch `docs/debrief-parser-streaming`) covers 1 and 9.
