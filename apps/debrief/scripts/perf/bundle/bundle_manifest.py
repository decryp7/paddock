"""Turns a per-file inventory of a diagnostic bundle into an anonymous manifest for gen_bundle.py.

usage: python3 -I bundle_manifest.py INVENTORY.md OUT.csv

The inventory is a Markdown file with `## <n>. <family>` sections and rows
`| path | bytes | size | kind | encoding | shape | stamped % | first line |`. It is not in this repository and may hold
real names, so nothing of a real name leaves this script: every path component is rewritten. Kept: the tree (folders,
files per folder), each component's length (so path lengths match), digit-run lengths and order, date-shaped runs, a
leading or trailing `~`, numbered suffixes (`.log.1`), and extensions on the neutral list. Also kept: family number, size,
kind, encoding, timestamp shape, share of stamped lines. The "first line" column is never read.
"""
import csv
import re
import sys

NEUTRAL_EXT = set(
    "evtx dmp log txt csv xml xaml config ini reg inf html js css bmp jpg png zip pol dll tmp old dat bin db xsd xslt".split()
)
GENERIC_WORDS = {"log", "logs", "old", "bak", "tmp", "temp", "trace", "error", "debug", "backup", "data", "system", "application", "security"}
KINDS = {
    "proprietary binary (not analysed)": "opaque",
    "text, timestamped lines": "stamped",
    "text, partly timestamped": "partly",
    "text, no line timestamps": "plain",
    "text (blank)": "blank",
    "XML / markup text": "xml",
    "BMP image": "bmp",
    "JPEG image": "jpeg",
    "PNG image": "png",
    "empty": "empty",
    "Windows minidump (MDMP)": "dump",
    "Windows PE (DLL/EXE)": "pe",
    "ZIP archive": "zip",
    "Windows event log (EVTX)": "evtx",
    "Group Policy registry.pol": "pol",
}


def rows(inventory):
    family = None
    with open(inventory, encoding="utf8") as f:
        for line in f:
            if line.startswith("## "):
                m = re.match(r"## (\d+)\.", line)
                family = int(m.group(1)) if m else None
            elif family and line.startswith("|") and not line.startswith(("|---", "| Path")):
                cells = [c.strip() for c in line.strip().strip("|").split("|")]
                if len(cells) >= 8:
                    yield family, cells


class Names:
    """Injective, deterministic replacement of the letters and digits of names."""

    def __init__(self):
        self.words, self.numbers = {}, {}

    def word(self, run):
        if run.lower() in GENERIC_WORDS:
            return run.lower()
        token = self.words.setdefault(run.lower(), "a%x" % len(self.words))
        return token + "x" * (len(run) - len(token))

    def number(self, run, rank):
        if len(run) == 8 and run.startswith("20"):
            return run
        return "%0*d" % (len(run), rank[(len(run), run)])

    def component(self, name, rank, is_file):
        ext = ""
        if is_file:
            m = re.match(r"^(.+?)(\.[A-Za-z0-9]{1,8}(?:~)?)?(\.\d{1,3})?$", name)
            stem, e, n = m.group(1), m.group(2) or "", m.group(3) or ""
            tilde = "~" if e.endswith("~") else ""
            e = e.rstrip("~").lower()
            if e and e[1:] not in NEUTRAL_EXT:
                e = ".dat" if len(e) % 2 else ".db"
            ext, name = e + tilde + n, stem
        out = re.sub(r"\d+", lambda m: self.number(m.group(0), rank), name)
        return re.sub(r"(?<![a-z0-9])[^\W\d_]+|[^\W\d_]+", lambda m: self.word(m.group(0)), out) + ext


def main(inventory, out):
    all_rows = list(rows(inventory))
    runs = sorted({(len(r), r) for _, c in all_rows for r in re.findall(r"\d+", c[0].strip("`"))})
    rank = {}
    per_len = {}
    for key in runs:
        per_len[key[0]] = per_len.get(key[0], 0) + 1
        rank[key] = per_len[key[0]] % (10 ** key[0])
    names = Names()
    seen = set()
    with open(out, "w", newline="", encoding="utf8") as f:
        w = csv.writer(f)
        w.writerow(["path", "family", "bytes", "kind", "encoding", "shape", "stamped"])
        for family, c in all_rows:
            parts = c[0].strip("`").replace("\\", "/").split("/")
            dirs = [names.component(p, rank, False) for p in parts[:-1]]
            path = "/".join(dirs + [names.component(parts[-1], rank, True)])
            while path.lower() in seen:  # two real extensions can map to one neutral extension
                path += "~"
            seen.add(path.lower())
            stamped = re.findall(r"\d+", c[6])
            w.writerow([path, family, c[1].replace(",", ""), KINDS[c[3]], c[4], c[5].strip("`"), stamped[0] if stamped else ""])


if __name__ == "__main__":
    main(*sys.argv[1:3])
