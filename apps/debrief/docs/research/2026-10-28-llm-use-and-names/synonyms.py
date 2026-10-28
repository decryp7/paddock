"""Synthetic check: can a static embedding (Model2Vec) map a user's synonym to a program name?

All names are invented or public Windows binary names; no user data. Entities are given as layer 1 would leave
them (segmented words); "pack" adds the one-line description a shipped knowledge pack would hold.
"""
import sys
import numpy as np
from model2vec import StaticModel

# (key, layer-1 words, pack description or None for in-house names a pack cannot know)
ENTS = [
    ("graphic builder", "graphic builder", None),
    ("report builder", "report builder", None),
    ("tray agent", "tray agent", None),
    ("font cache", "font cache", None),
    ("cache cleaner", "cache cleaner", None),
    ("update service", "update service", None),
    ("license manager", "license manager", None),
    ("sync client", "sync client", None),
    ("backup agent", "backup agent", None),
    ("crash reporter", "crash reporter", None),
    ("data importer", "data importer", None),
    ("chart viewer", "chart viewer", None),
    ("mail connector", "mail connector", None),
    ("scan station", "scan station", None),
    ("audio router", "audio router", None),
    ("video recorder", "video recorder", None),
    ("inventory tracker", "inventory tracker", None),
    ("label printer", "label printer", None),
    ("svchost", "svchost", "Service Host, runs Windows services"),
    ("lsass", "lsass", "Local Security Authority, login and security policy"),
    ("spoolsv", "spoolsv", "Print Spooler, printer queue"),
    ("wuauclt", "wuauclt", "Windows Update client"),
    ("winlogon", "winlogon", "Windows Logon, sign in"),
    ("msedge", "msedge", "Microsoft Edge web browser"),
    ("msiexec", "msiexec", "Windows Installer, installs MSI packages"),
    ("dwm", "dwm", "Desktop Window Manager, compositor"),
    ("searchindexer", "search indexer", "Windows Search indexer"),
    ("msmpeng", "msmpeng", "Microsoft Defender Antivirus engine"),
    ("explorer", "explorer", "Windows Explorer, desktop shell and file manager"),
    ("onedrive", "onedrive", "OneDrive cloud file sync"),
]

QUERIES = [
    ("image editor", "graphic builder"), ("picture tool", "graphic builder"), ("drawing program", "graphic builder"),
    ("pdf export", "report builder"), ("notification area", "tray agent"), ("typeface cache", "font cache"),
    ("temp file remover", "cache cleaner"), ("patch installer", "update service"), ("activation", "license manager"),
    ("file sync", "sync client"), ("restore", "backup agent"), ("error report", "crash reporter"),
    ("csv import", "data importer"), ("graph viewer", "chart viewer"), ("email", "mail connector"),
    ("scanner", "scan station"), ("sound", "audio router"), ("screen capture", "video recorder"),
    ("stock levels", "inventory tracker"), ("barcode printing", "label printer"),
    ("printer queue", "spoolsv"), ("antivirus", "msmpeng"), ("windows update", "wuauclt"), ("sign in", "winlogon"),
    ("web browser", "msedge"), ("installer", "msiexec"), ("window compositor", "dwm"), ("search", "searchindexer"),
    ("security policy", "lsass"), ("file manager", "explorer"), ("cloud storage", "onedrive"),
    ("service host", "svchost"),
]
NEGATIVES = ["banana", "weather forecast", "tax return", "football score", "holiday photos", "recipe"]


def run(model_name: str, use_pack: bool):
    m = StaticModel.from_pretrained(model_name)
    texts = [w if (not use_pack or d is None) else f"{w} {d}" for _, w, d in ENTS]
    E = m.encode(texts)
    E = E / np.linalg.norm(E, axis=1, keepdims=True)
    keys = [k for k, _, _ in ENTS]

    def rank(q):
        v = m.encode([q])[0]
        v = v / (np.linalg.norm(v) or 1)
        s = E @ v
        order = np.argsort(-s)
        return [(keys[i], float(s[i])) for i in order]

    top1 = top3 = 0
    inhouse = [0, 0, 0]
    known = [0, 0, 0]
    right_scores = []
    for q, want in QUERIES:
        r = rank(q)
        ids = [k for k, _ in r]
        hit1, hit3 = ids[0] == want, want in ids[:3]
        top1 += hit1
        top3 += hit3
        bucket = known if dict((k, d) for k, _, d in ENTS)[want] else inhouse
        bucket[0] += 1
        bucket[1] += hit1
        bucket[2] += hit3
        right_scores.append(dict(r)[want])
    neg_max = [rank(n)[0][1] for n in NEGATIVES]
    n = len(QUERIES)
    print(f"{model_name:32} pack={use_pack!s:5} top1 {top1}/{n} top3 {top3}/{n} | in-house top1 {inhouse[1]}/{inhouse[0]} "
          f"top3 {inhouse[2]}/{inhouse[0]} | well-known top1 {known[1]}/{known[0]} top3 {known[2]}/{known[0]} | "
          f"right-cos median {np.median(right_scores):.2f} | negatives' best cos median {np.median(neg_max):.2f} max {max(neg_max):.2f}")
    if "-v" in sys.argv:
        for q, want in QUERIES:
            r = rank(q)[:3]
            print(f"   {q:20} want {want:18} got {[(k, round(s, 2)) for k, s in r]}")


for name in ["minishlab/potion-base-8M", "minishlab/potion-retrieval-32M", "minishlab/potion-base-32M"]:
    for pack in (False, True):
        run(name, pack)
