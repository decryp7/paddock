"""Non-LLM layers on ADR 0053's own invented eval set (crates/debrief-core/tests/it/names_eval.rs, ENTS/NEGATIVES).

1. Deterministic abbreviation rule: the query, without spaces, splits in order into one piece per entity word, each
   piece starting with the word's first letter and being a subsequence of the word (or a table abbreviation).
2. Model2Vec cosine for the synonyms, reported as top-1 / top-3 and against the negatives' best score.
"""
import functools
import numpy as np
from model2vec import StaticModel

ENTS = [
    ("graphic builder", "gfxbldr", "image editor"), ("tray agent", "tryagnt", "system tray helper"),
    ("report builder", "rptbldr", "pdf generator"), ("print spooler", "prtspl", "printing service"),
    ("update helper", "updhlp", "auto updater"), ("sync worker", "synwrk", "cloud sync"),
    ("crash reporter", "crshrpt", "error uploader"), ("media player", "mplayr", "video viewer"),
    ("backup agent", "bkpagt", "archive service"), ("license checker", "lickchk", "activation validator"),
    ("config loader", "cfgldr", "settings reader"), ("network monitor", "netmon", "traffic watcher"),
    ("disk indexer", "dskidx", "file crawler"), ("event collector", "evtcol", "log gatherer"),
    ("session broker", "sessbrk", "login manager"), ("video encoder", "videnc", "movie converter"),
    ("search service", "srchsvc", "finder daemon"), ("font cache", "fntcache", "glyph store"),
    ("thumbnail host", "thumbhst", "preview renderer"), ("device watcher", "devwtch", "hardware sentinel"),
    ("spell checker", "splchk", "grammar tool"), ("cache cleaner", "cchcln", "temp purger"),
    ("mail relay", "mlrelay", "smtp forwarder"), ("window manager", "wndmgr", "desktop compositor"),
    ("audio player", "", ""), ("event monitor", "", ""), ("task manager", "", ""), ("search indexer", "", ""),
]
NEGATIVES = ["quantum flux", "banana", "database", "zzzz", "red balloon", "xylophone", "kernel panic", "weather report",
             "coffee machine", "blue sky", "lorem ipsum", "monkey", "violin", "hockey stick", "mountain", "tablecloth",
             "grape builder", "tree agent"]
# A small table of conventional software abbreviations whose letters are not a subsequence of the word.
TABLE = {"gfx": "graphic", "lic": "license", "xfer": "transfer", "pkg": "package"}


def subseq(p, w):
    it = iter(w)
    return all(c in it for c in p)


def piece_ok(p, w):
    if TABLE.get(p) == w or (w.endswith("s") and TABLE.get(p) == w[:-1]):
        return True
    return len(p) >= 1 and p[0] == w[0] and subseq(p, w)


def abbrev_match(q, words):
    q = q.replace(" ", "").replace("-", "").lower()

    @functools.lru_cache(None)
    def go(i, k):
        if k == len(words):
            return i == len(q)
        for j in range(i + 1, len(q) + 1):
            if piece_ok(q[i:j], words[k]) and go(j, k + 1):
                return True
        return False

    return go(0, 0)


def abbrev_rank(q):
    hits = [e[0] for e in ENTS if abbrev_match(q, tuple(e[0].split()))]
    # ties: the shorter entity (tighter match) first
    return sorted(hits, key=lambda h: len(h.replace(" ", "")) - len(q))


ab = [(e[1], e[0]) for e in ENTS if e[1]]
ok1 = sum(abbrev_rank(q)[:1] == [w] for q, w in ab)
amb = sum(len(abbrev_rank(q)) > 1 for q, _ in ab)
neg = [n for n in NEGATIVES if abbrev_rank(n)]
print(f"abbreviation rule: top-1 {ok1}/{len(ab)}, ambiguous (more than one entity) {amb}, negatives matched {neg}")
for q, w in ab:
    r = abbrev_rank(q)
    if r[:1] != [w]:
        print(f"   miss {q!r}: want {w!r} got {r}")

for name in ["minishlab/potion-base-8M", "minishlab/potion-base-32M", "minishlab/potion-retrieval-32M"]:
    m = StaticModel.from_pretrained(name)
    keys = [e[0] for e in ENTS]
    E = m.encode(keys)
    E = E / np.linalg.norm(E, axis=1, keepdims=True)

    def rank(q):
        v = m.encode([q])[0]
        v = v / (np.linalg.norm(v) or 1)
        s = E @ v
        o = np.argsort(-s)
        return [(keys[i], float(s[i])) for i in o]

    syn = [(e[2], e[0]) for e in ENTS if e[2]]
    t1 = sum(rank(q)[0][0] == w for q, w in syn)
    t3 = sum(w in [k for k, _ in rank(q)[:3]] for q, w in syn)
    right = [dict(rank(q))[w] for q, w in syn]
    best_neg = sorted(rank(n)[0][1] for n in NEGATIVES)
    # precision at a threshold that rejects every negative except lexical ones ("grape builder", "tree agent")
    thr = 0.45
    conf = [(q, w) for q, w in syn if rank(q)[0][1] >= thr]
    conf_ok = sum(rank(q)[0][0] == w for q, w in conf)
    negs_over = [n for n in NEGATIVES if rank(n)[0][1] >= thr]
    print(f"{name:30} synonyms top-1 {t1}/{len(syn)} top-3 {t3}/{len(syn)}; right cos median {np.median(right):.2f}; "
          f"negatives' best cos median {np.median(best_neg):.2f} max {best_neg[-1]:.2f}; at cos>={thr}: "
          f"{len(conf)} answered, {conf_ok} right, negatives over {negs_over}")
