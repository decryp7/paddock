# Regenerate the two committed SVGs: python3 apps/debrief/docs/research/img/src/duckdb-write-paths.py apps/debrief/docs/research/img
"""Draws the two write-path diagrams to diagrams/STYLE.md tokens (Debrief roles). Output dir = argv[1]."""
import sys
OUT = sys.argv[1]
STYLE = """  <style>
    /* diagrams/STYLE.md tokens, Debrief roles. Light by default, dark under prefers-color-scheme. */
    .bg { fill: #ffffff; }
    text { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif; font-size: 12px; fill: #14171C; }
    .mono { font-family: ui-monospace, "SF Mono", Menlo, "Cascadia Mono", Consolas, monospace; }
    .muted { fill: #565E6B; }
    .title { font-size: 24px; font-weight: 600; }
    .kind { font-size: 14px; }
    .name { font-size: 14px; font-weight: 600; }
    .glabel { font-weight: 600; fill: #565E6B; }
    .foot { font-size: 12px; fill: #565E6B; }
    .group { fill: #F8F9FA; stroke: #C4C7CD; stroke-width: 1; }
    .group-ext { fill: #F8F9FA; stroke: #6B7380; stroke-width: 1; stroke-dasharray: 6 4; }
    .n-own { fill: #E8EFFA; stroke: #1D5FD1; stroke-width: 1.5; }
    .n-proc { fill: #F0F2F5; stroke: #565E6B; stroke-width: 1.5; }
    .n-ext { fill: #F4F5F7; stroke: #6B7380; stroke-width: 1.5; stroke-dasharray: 6 4; }
    .legend { fill: none; stroke: #C4C7CD; stroke-width: 1; }
    .edge { stroke: #565E6B; stroke-width: 1.5; fill: none; }
    .dash { stroke-dasharray: 6 4; }
    .head { fill: #565E6B; }
    .ohead { fill: none; stroke: #565E6B; stroke-width: 1.5; }
    .t-own { fill: #1D5FD1; }
    .b-own { fill: #1D5FD1; }
    .b-cap { fill: #1D5FD1; fill-opacity: 0.35; stroke: #1D5FD1; stroke-width: 1.5; }
    .b-proc { fill: #565E6B; }
    .b-ext { fill: #6B7380; }
    .grid { stroke: #C4C7CD; stroke-width: 1; }
    .halo { paint-order: stroke; stroke: #ffffff; stroke-width: 4px; stroke-linejoin: round; }
    @media (prefers-color-scheme: dark) {
      .bg { fill: #15171B; }
      text { fill: #ECEEF1; }
      .muted, .glabel, .foot { fill: #A0A7B2; }
      .group { fill: #121418; stroke: #353A42; }
      .group-ext { fill: #121418; stroke: #7F8794; }
      .n-own { fill: #202C3F; stroke: #5B9BFF; }
      .n-proc { fill: #1B1E23; stroke: #A0A7B2; }
      .n-ext { fill: #17191E; stroke: #7F8794; }
      .legend { stroke: #353A42; }
      .edge { stroke: #A0A7B2; }
      .head { fill: #A0A7B2; }
      .ohead { stroke: #A0A7B2; }
      .t-own, .b-own, .b-cap { fill: #5B9BFF; }
      .b-cap { stroke: #5B9BFF; }
      .b-proc { fill: #A0A7B2; }
      .b-ext { fill: #7F8794; }
      .grid { stroke: #353A42; }
      .halo { stroke: #15171B; }
    }
  </style>"""
MARKERS = """  <defs>
    <marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M0,0 L10,5 L0,10 z" class="head"/></marker>
    <marker id="oh" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto"><path d="M1,1 L9,5 L1,9" class="ohead"/></marker>
  </defs>"""

def esc(s): return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
def text(x, y, s, cls="", anchor=None):
    a = f' text-anchor="{anchor}"' if anchor else ""
    c = f' class="{cls}"' if cls else ""
    return f'    <text x="{x}" y="{y}"{c}{a}>{esc(s)}</text>'
def node(x, y, w, h, cls, name, lines=(), mono_name=False):
    out = [f'    <rect x="{x}" y="{y}" width="{w}" height="{h}" rx="6" class="{cls}"/>']
    n = 1 + len(lines)
    top = y + h / 2 - (n - 1) * 8 + 5
    cx = x + w / 2
    out.append(text(cx, round(top), name, "name mono" if mono_name else "name", "middle"))
    for i, (s, mono) in enumerate(lines):
        out.append(text(cx, round(top + 16 * (i + 1)), s, "mono muted" if mono else "muted", "middle"))
    return out
def edge(d, dashed=False):
    return f'    <path d="{d}" class="edge{" dash" if dashed else ""}" marker-end="url(#{"oh" if dashed else "ah"})"/>'

def header(w, h, tid, title, desc):
    return [f'<?xml version="1.0" encoding="UTF-8"?>',
            f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}" role="img" aria-labelledby="{tid}t {tid}d">',
            f'  <title id="{tid}t">{esc(title)}</title>', f'  <desc id="{tid}d">{esc(desc)}</desc>', STYLE, MARKERS,
            f'  <rect class="bg" x="0" y="0" width="{w}" height="{h}"/>']

# ---------------------------------------------------------------- architectures
W, H = 1152, 984
C1, C2, C3, C4, NW = 72, 256, 440, 640, (144, 144, 160, 160)
NX = 832
o = header(W, H, "d1", "Write architectures compared",
    "Three ways to write events. A, single writer: 8 parse threads queue on a channel into one Appender that writes "
    "events.duckdb; 1 Appender 200k events/s, 4 Appenders 355k in the Quack harness, a 2nd 1.0x on the writer bench (indicative). "
    "B, fan-out (Debrief today): each parse thread has its own Parquet writer and segment file; catalog.duckdb holds "
    "the segment list and a view that reads them; 8 writers 4.38M events/s in the Quack harness. C, server mode: 4 "
    "client processes send rows over a socket to a DuckDB Quack server process; 1 client 115k events/s, 4 clients "
    "416k using 4.6 GB at a 1 GB limit, out of memory in 2 of 3 rounds at 192 MB.")
o += [text(48, 64, "Write architectures compared", "title"),
      text(48, 88, "Flow · Debrief storage engine, import write path", "kind muted"),
      text(48, 108, "The same 8 parse threads feed each design; the difference is where they wait. Numbers: Quack harness, same batches, Mac mini, 2026-10-28.", "muted")]
# Panel A
o += ['    <rect x="48" y="136" width="1056" height="144" rx="10" class="group"/>',
      text(72, 160, "A · Single writer into a native table", "glabel"),
      text(72, 178, "INSERT or the Appender; Debrief's import before R2b (2026-10-20)", "muted")]
y = 200
o += node(C1, y, NW[0], 56, "n-proc", "Parse threads", [("[8 threads]", False)])
o += node(C2, y, NW[1], 56, "n-proc", "Channel", [("threads wait here", False)])
o += node(C3, y, NW[2], 56, "n-proc", "1 Appender", [("long-lived, large batches", False)])
o += node(C4, y, NW[3], 56, "n-proc", "events.duckdb", [("compressed as written", False)], mono_name=True)
o += [edge(f"M{C1+NW[0]},228 L{C2-2},228"), edge(f"M{C2+NW[1]},228 L{C3-2},228"), edge(f"M{C3+NW[2]},228 L{C4-2},228")]
o += [text(NX, 212, "200k ev/s, 1 Appender", "name"), text(NX, 230, "4 Appenders: 1.8× at about 2× memory", "muted"),
      text(NX, 246, "2 Appenders: 1.0× on the writer bench", "muted"), text(NX, 262, "(indicative, unrecorded)", "muted")]
# Panel B
o += ['    <rect x="48" y="336" width="1056" height="328" rx="10" class="group"/>',
      text(72, 360, "B · Fan-out: one Parquet file per thread, a view over them", "glabel"),
      text(72, 378, "Debrief's import today (ADR 0014)", "muted")]
lanes = [(400, "[1 of 8]"), (480, "[2 of 8]"), (560, "[8 of 8]")]
for ly, tag in lanes:
    o += node(C1, ly, NW[0], 56, "n-proc", "Parse thread", [(tag, False)])
    o += node(C2, ly, NW[1], 56, "n-own", "Parquet writer", [("[ArrowWriter, zstd]", False)])
    o += node(C3, ly, NW[2], 56, "n-proc", "Segment file", [("seg-*.parquet", True)])
    c = ly + 28
    o += [edge(f"M{C1+NW[0]},{c} L{C2-2},{c}"), edge(f"M{C2+NW[1]},{c} L{C3-2},{c}")]
o += node(C4, 456, NW[3], 104, "n-own", "catalog.duckdb", [("segment list +", False), ("VIEW events AS", True), ("read_parquet([...])", True)], mono_name=True)
o += [edge(f"M{C4},480 L620,480 L620,428 L{C3+NW[2]+2},428", True),
      edge(f"M{C4},508 L{C3+NW[2]+2},508", True),
      edge(f"M{C4},536 L620,536 L620,588 L{C3+NW[2]+2},588", True)]
o += [text(C1 + NW[0]/2, 553, "…", "muted", "middle"), text(C2 + NW[1]/2, 553, "…", "muted", "middle"), text(C3 + NW[2]/2, 553, "…", "muted", "middle")]
o += [text(336, 640, "written by the parquet crate, not DuckDB; temp name, then rename", "muted", "middle")]
o += [text(NX, 470, "4.38M ev/s, 8 writers", "name t-own"), text(NX, 488, "12 writers: 4.41M (no gain)", "muted"),
      text(NX, 504, "R2b, Windows VM: 1 → 8 writers ≈ 4×", "muted"), text(NX, 520, "no shared lock while writing", "muted")]
# Panel C
o += ['    <rect x="48" y="720" width="1056" height="144" rx="10" class="group-ext"/>',
      text(72, 744, "C · Server mode: clients send rows over the Quack protocol", "glabel"),
      text(72, 762, "Evaluated 2026-10-28; not adopted (ADR 0010, “do not migrate”)", "muted")]
y = 784
o += node(C1, y, NW[0], 56, "n-proc", "Clients", [("[4 processes]", False)])
o += node(C2, y, NW[1], 56, "n-ext", "Socket", [("encode, copy, decode", False)])
o += node(C3, y, NW[2], 56, "n-ext", "Quack server", [("buffers rows per client", False)])
o += node(C4, y, NW[3], 56, "n-proc", "events.duckdb", [("server's database", False)], mono_name=True)
o += [edge(f"M{C1+NW[0]},812 L{C2-2},812"), edge(f"M{C2+NW[1]},812 L{C3-2},812"), edge(f"M{C3+NW[2]},812 L{C4-2},812")]
o += [text(NX, 796, "115k ev/s, 1 client (Quack harness)", "name"), text(NX, 814, "4 clients: 416k, 4.6 GB at a 1 GB limit", "muted"),
      text(NX, 830, "192 MB: out of memory in 2 of 3 rounds", "muted")]
# Legend and footer
o += ['    <rect x="48" y="896" width="744" height="40" rx="14" class="legend"/>',
      '    <rect x="64" y="908" width="24" height="16" rx="4" class="n-own"/>', text(96, 920, "Debrief's design"),
      '    <rect x="208" y="908" width="24" height="16" rx="4" class="n-proc"/>', text(240, 920, "process, data"),
      '    <rect x="344" y="908" width="24" height="16" rx="4" class="n-ext"/>', text(376, 920, "external (DuckDB server)"),
      '    <path d="M528,916 L568,916" class="edge" marker-end="url(#ah)"/>', text(576, 920, "rows"),
      '    <path d="M632,916 L672,916" class="edge dash" marker-end="url(#oh)"/>', text(680, 920, "view reads"),
      text(1104, 920, "Debrief · updated 2026-10-29", "foot", "end"), "</svg>", ""]
open(f"{OUT}/duckdb-write-architectures.svg", "w").write("\n".join(o))

# ---------------------------------------------------------------- throughput
W, H = 1152, 624
X0, PX = 400, 120  # 1M events/s = 120 px
def bx(v): return round(v / 1e6 * PX, 1)
o = header(W, H, "d2", "Write throughput by method",
    "Events per second, linear scale, in two groups that are not comparable. Write path only, Quack harness, same 3M "
    "parsed batches, medians: Quack server 1 client 115k, 1 Appender 200k, 4 Appenders 355k, Parquet 8 writers 4.38M, "
    "12 writers 4.41M. Full import with parsing: DuckDB 2.0 at load 33-65, 685k to 991k (2026-10-28, 16.8M events); "
    "DuckDB 1.5.6 at load 20-90, 1.0M to 1.55M (2026-10-27, 56M events).")
o += [text(48, 64, "Write throughput by method", "title"),
      text(48, 88, "Bar chart · events per second, linear scale, Mac mini", "kind muted"),
      text(48, 108, "Two groups that must not be compared with each other: the write path alone, and the full import with parsing.", "muted")]
def grid(y0, y1):
    return [f'    <line x1="{X0 + i*PX}" y1="{y0}" x2="{X0 + i*PX}" y2="{y1}" class="grid"/>' for i in range(6)]
o += [text(48, 152, "Write path only · Quack harness, the same 3M parsed batches, 2026-10-28, medians", "glabel")]
o += grid(164, 368)
rows1 = [("Quack server, 1 client", 115e3, "n-ext", "115k"), ("Appender, 1", 200e3, "b-proc", "200k"),
         ("Appenders, 4", 355e3, "b-proc", "355k"), ("Parquet, 8 writers", 4.38e6, "b-own", "4.38M"),
         ("Parquet, 12 writers", 4.41e6, "b-own", "4.41M")]
for i, (lab, v, cls, val) in enumerate(rows1):
    y = 172 + 40 * i
    o += [text(48, y + 17, lab, "kind"), f'    <rect x="{X0}" y="{y}" width="{bx(v)}" height="24" rx="2" class="{cls}"/>',
          text(round(X0 + bx(v) + 8), y + 16, val, "halo")]
o += [text(48, 400, "Full import, parse + write + catalog · performance.md, not comparable with the group above", "glabel")]
o += grid(412, 496)
rows2 = [("Import, DuckDB 2.0, load 33–65", 685e3, 991e3, "685k–991k"), ("Import, DuckDB 1.5.6, load 20–90", 1.0e6, 1.55e6, "1.0–1.55M")]
for i, (lab, lo, hi, val) in enumerate(rows2):
    y = 420 + 40 * i
    o += [text(48, y + 17, lab, "kind"), f'    <rect x="{X0}" y="{y}" width="{bx(lo)}" height="24" rx="2" class="b-own"/>',
          f'    <rect x="{round(X0 + bx(lo), 1)}" y="{y}" width="{round(bx(hi) - bx(lo), 1)}" height="24" rx="2" class="b-cap"/>',
          text(round(X0 + bx(hi) + 8), y + 16, val, "halo")]
o += [text(X0 + i*PX, 516, l, "muted", "middle") for i, l in enumerate(["0", "1M", "2M", "3M", "4M", "5M"])]
o += [text(X0 + 300, 536, "events per second", "muted", "middle")]
o += ['    <rect x="48" y="560" width="816" height="40" rx="14" class="legend"/>',
      '    <rect x="64" y="572" width="24" height="16" rx="2" class="b-own"/>', text(96, 584, "Parquet fan-out (Debrief)"),
      '    <rect x="264" y="572" width="24" height="16" rx="2" class="b-proc"/>', text(296, 584, "Appender"),
      '    <rect x="376" y="572" width="24" height="16" rx="2" class="n-ext"/>', text(408, 584, "Quack server"),
      '    <rect x="512" y="572" width="24" height="16" rx="2" class="b-cap"/>', text(544, 584, "range: solid to the low end, faded from low to high"),
      text(1104, 584, "Debrief · updated 2026-10-29", "foot", "end"), "</svg>", ""]
open(f"{OUT}/duckdb-write-throughput.svg", "w").write("\n".join(o))
print("ok")
