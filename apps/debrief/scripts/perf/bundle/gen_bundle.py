"""Writes an invented diagnostic bundle of the shape a manifest describes (see bundle_manifest.py).

usage: python3 -I gen_bundle.py MANIFEST.csv OUT_DIR SECURITY_EVTX DUMP_FULL DUMP_SMALL

Every byte is invented and deterministic (seeded by the path). Names are generic; the content names no vendor, product,
site, station or account. What is imitated, per file: exact size, kind, encoding, timestamp shape, share of stamped lines.
- `evtx`: the fixture's chunk filled with copies (as crates/debrief-plugins/tests/it/evtx_memory.rs does), valid for the
  parser, about 130 events per 64 KiB. Event times repeat the fixture's.
- `dump`: a valid sample minidump, padded to size with invented memory (a real full dump is mostly memory ranges).
- `opaque`: invented binary with NUL bytes and no public magic number, a share with text-like stretches. No real format.
"""
import csv
import datetime as dt
import os
import random
import struct
import sys
import zipfile

MTIME = dt.datetime(2026, 6, 17, 9, 0, tzinfo=dt.timezone.utc).timestamp()
WORDS = (
    "alpha bravo charlie delta echo foxtrot golf hotel india juliet kilo lima mike november oscar papa quebec romeo sierra "
    "tango uniform victor whiskey xray yankee zulu service worker queue module driver buffer session request reply timeout "
    "retry connect disconnect started stopped failed ready pending memory handle thread window device channel config update"
).split()
LEVELS = ["INFO", "INFO", "INFO", "WARN", "ERROR", "DEBUG"]
HEADER = 4096
CHUNK = 65536


def sentence(r, low=4, high=14):
    return " ".join(r.choices(WORDS, k=r.randint(low, high)))


def fmt_time(shape, t, r):
    ms = r.randrange(1000)
    off = "+01:00" if t.month in (1, 2, 3, 11, 12) else "+02:00"
    if shape.startswith("yyyy/MM/dd,"):
        return t.strftime(f"%Y/%m/%d,%H:%M:%S.{ms:03d}{off}")
    if shape == "yyyy/MM/dd HH:mm:ss +zz:zz":
        return t.strftime(f"%Y/%m/%d %H:%M:%S {off}")
    if shape == "yyyy/MM/dd HH:mm:ss.fff +zz:zz":
        return t.strftime(f"%Y/%m/%d %H:%M:%S.{ms:03d} {off}")
    if shape == "yyyy/MM/dd HH:mm:ss":
        return t.strftime("%Y/%m/%d %H:%M:%S")
    if shape == "yyyy-MM-dd HH:mm:ss":
        return t.strftime("%Y-%m-%d %H:%M:%S")
    if shape.startswith("[yyyy-MM-dd"):
        return t.strftime(f"[%Y-%m-%d %H:%M:%S,{ms * 1000:06d}]")
    if shape == "MM/dd/yyyy HH:mm:ss":
        return t.strftime("%m/%d/%Y %H:%M:%S")
    if shape.startswith("Ddd Mon"):
        return f"{t:%a %b} {t.day} {t:%H:%M:%S %Y}"
    if shape.startswith("MSI"):
        return f"MSI (s) ({r.randrange(256):02X}:{r.randrange(256):02X}) [{t:%H:%M:%S}:{ms:03d}]:"
    if shape.startswith("Date,"):
        return t.strftime("Date,%Y.%m.%d,%H:%M:%S,Uptime,12345")
    if shape == "yyyy.MM.dd HH:mm:ss.fff":
        return t.strftime(f"%Y.%m.%d %H:%M:%S.{ms:03d} UTC=+1:00")
    return t.strftime("%Y/%m/%d %H:%M:%S")


def stamped_line(shape, t, r, seq):
    stamp = fmt_time(shape, t, r)
    if shape.startswith("yyyy/MM/dd,"):
        # CSV; a newline inside a message is written as two underscores.
        msg = sentence(r)
        if seq % 37 == 0:
            msg += "__   at module.worker.run()__   at module.queue.next()"
        return f'{stamp},{r.choice(LEVELS)},{1000 + r.randrange(9000)},{r.randrange(64)},"{msg}"'
    if shape.startswith("Ddd Mon"):
        return f"{stamp};0x{r.randrange(1 << 16):08X};{r.choice(LEVELS)};source{r.randrange(40)};{sentence(r)}"
    if shape.startswith("Date,"):
        return stamp
    return f"{stamp} {r.choice(LEVELS)} {sentence(r)}"


def continuation(shape, r):
    if shape.startswith("Date,"):
        return f"proc{r.randrange(300)},{r.randrange(1 << 20)},{r.randrange(1 << 16)},{r.randrange(100)}"
    return f"   at module.{r.choice(WORDS)}.{r.choice(WORDS)}() line {r.randrange(9000)}"


def encoder(enc, r):
    if enc == "UTF-16LE BOM":
        return b"\xff\xfe", lambda s: s.encode("utf-16-le")
    if enc == "UTF-16LE no BOM":
        return b"", lambda s: s.encode("utf-16-le")
    if enc == "UTF-8 BOM":
        return b"\xef\xbb\xbf", lambda s: s.encode("utf8")
    if enc == "UTF-8":
        return b"", lambda s: (s.replace("alpha", "alphä").replace("echo", "écho")).encode("utf8")
    if enc.startswith("8-bit"):
        return b"", lambda s: (s.replace("alpha", "alphä").replace("echo", "écho")).encode("cp1252")
    return b"", lambda s: s.encode("ascii")


def write_lines(f, size, enc, r, next_line):
    bom, encode = encoder(enc, r)
    f.write(bom)
    left = size - len(bom)
    seq = 0
    while left > 0:
        batch = []
        got = 0
        target = min(left, 1 << 20)
        while got < target:
            line = next_line(seq) + "\r\n"
            seq += 1
            got += len(line) * (2 if enc.startswith("UTF-16") else 1)
            batch.append(line)
        data = encode("".join(batch))
        if len(data) > left:
            unit = 2 if enc.startswith("UTF-16") else 1
            data = data[: left - left % unit]
            f.write(data)
            f.write(b"\n" * (left - len(data)))
            return
        f.write(data)
        left -= len(data)


def text_file(f, row, r):
    size, enc, shape = row["size"], row["encoding"], row["shape"]
    kind = row["kind"]
    if kind in ("blank",):
        f.write(b"\r\n \r\n"[:size])
        return
    if not shape:
        if size > (100 << 20) and enc.startswith("UTF-16"):  # a registry export
            head = "Windows Registry Editor Version 5.00\r\n\r\n"

            def reg(seq):
                if seq == 0:
                    return head.rstrip()
                if seq % 8 == 1:
                    return f"\r\n[HKEY_LOCAL_MACHINE\\SOFTWARE\\{r.choice(WORDS)}\\{r.choice(WORDS)}\\{seq // 8}]"
                return f'"{r.choice(WORDS)}{seq % 8}"=dword:{r.randrange(1 << 32):08x}'

            write_lines(f, size, enc, r, reg)
        else:
            write_lines(f, size, enc, r, lambda seq: f"{r.choice(WORDS)}={r.randrange(100000)} {r.choice(WORDS)}")
        return
    stamped = int(row["stamped"] or 100) / 100
    t = dt.datetime(2026, 3, 1) + dt.timedelta(seconds=r.randrange(5_000_000))
    # A file ends about at its size whatever the rate: a second or so per line.
    def line(seq):
        nonlocal t
        if r.random() < stamped:
            t += dt.timedelta(seconds=r.random() * 2)
            return stamped_line(shape, t, r, seq)
        return continuation(shape, r)

    write_lines(f, size, enc, r, line)


def xml_file(f, row, r):
    size, enc = row["size"], row["encoding"]
    bom, encode = encoder(enc, r)
    head = '<?xml version="1.0" encoding="utf-8"?>\r\n<root>\r\n'
    tail = "</root>\r\n"
    budget = size - len(bom) - (len(head) + len(tail)) * (2 if enc.startswith("UTF-16") else 1)
    if budget < 0:
        f.write((bom + encode(head + tail))[:size])
        return
    f.write(bom)
    f.write(encode(head))

    def item(seq):
        return f'  <item id="{seq}" name="{r.choice(WORDS)}" x="{r.randrange(4096)}" y="{r.randrange(4096)}" text="{sentence(r, 2, 6)}"/>'

    write_lines(f, budget, "ASCII" if not enc.startswith("UTF-16") else "UTF-16LE no BOM", r, item)
    f.write(encode(tail))


def filler(f, size, r):
    """Invented bytes: compressible runs and noise, as memory and images are."""
    while size > 0:
        n = min(size, 1 << 20)
        f.write((r.randbytes(n // 4) + bytes(n - n // 4)) if r.random() < 0.7 else r.randbytes(n))
        size -= n


def opaque_file(f, size, r):
    """NUL bytes, no public magic number; a third carry text-like stretches near the start."""
    wrote = 0
    if r.random() < 0.34 and size > 64:
        words = sentence(r, 3, 30).encode("ascii")
        if r.random() < 0.5:
            words = words.decode().encode("utf-16-le")
        head = bytes(r.randrange(2, 16)) + words
        head = head[:size]
        f.write(head)
        wrote = len(head)
    else:
        head = b"\x07" + bytes(r.randrange(1, 8))
        head = head[:size]
        f.write(head)
        wrote = len(head)
    left = size - wrote
    while left > 0:
        n = min(left, 1 << 16)
        pick = r.random()
        if pick < 0.5:
            f.write(bytes(n))
        else:
            b = bytearray(r.randbytes(n))
            b[::7] = bytes(len(b[::7]))  # never free of NUL bytes
            f.write(b)
        left -= n


def bmp_file(f, size, r):
    if size < 54:
        f.write(bytes(size))
        return
    f.write(b"BM" + struct.pack("<IHHI", size, 0, 0, 54) + struct.pack("<IiiHHIIiiII", 40, 640, 480, 1, 24, 0, size - 54, 2835, 2835, 0, 0))
    filler(f, size - 54, r)


def signed_file(f, size, magic, r):
    f.write(magic[:size])
    filler(f, max(0, size - len(magic)), r)


def zip_file(path, size, r):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_STORED) as z:
        z.writestr("readme.txt", sentence(r).encode())
        z.writestr("payload.dat", r.randbytes(max(0, size - 400)))
    have = os.path.getsize(path)
    if have < size:  # a zip may carry a comment up to 64 KiB; the rest is padding in the entry
        with zipfile.ZipFile(path, "a") as z:
            z.comment = bytes(min(size - have, 65535))
    return


class Evtx:
    """Dense chunks from the sample, as evtx_memory.rs builds them."""

    def __init__(self, sample_path):
        s = open(sample_path, "rb").read()
        self.header = s[:HEADER]
        c = bytearray(s[HEADER : HEADER + CHUNK])
        u32 = lambda at: struct.unpack_from("<I", c, at)[0]
        free = u32(0x30)
        records, at = [], 512
        while at < free:
            records.append(at)
            at += u32(at + 4)
        reusable = [(x, u32(x + 4)) for x in records[-2:]]
        end = free
        full = False
        while not full:
            for frm, ln in reusable:
                if end + ln > CHUNK:
                    full = True
                    break
                c[end : end + ln] = c[frm : frm + ln]
                records.append(end)
                end += ln
        struct.pack_into("<I", c, 0x2C, records[-1])
        struct.pack_into("<I", c, 0x30, end)
        self.chunk, self.records = bytes(c), records

    def write(self, f, size):
        chunks = max(1, (size - HEADER) // CHUNK)
        f.write(self.header)
        rid = 1
        for _ in range(chunks):
            c = bytearray(self.chunk)
            first = rid
            for x in self.records:
                struct.pack_into("<Q", c, x + 8, rid)
                rid += 1
            for at, v in ((8, first), (0x10, rid - 1), (0x18, first), (0x20, rid - 1)):
                struct.pack_into("<Q", c, at, v)
            f.write(c)
        return rid - 1


def main(manifest, out, sample_evtx, dump_full, dump_small):
    evtx = Evtx(sample_evtx)
    dumps = (open(dump_full, "rb").read(), open(dump_small, "rb").read())
    total = files = 0
    with open(manifest, encoding="utf8", newline="") as m:
        for row in csv.DictReader(m):
            row["size"] = size = int(row["bytes"])
            path = os.path.join(out, row["path"])
            os.makedirs(os.path.dirname(path), exist_ok=True)
            r = random.Random(row["path"])
            kind = row["kind"]
            if kind == "zip":
                zip_file(path, size, r)
            else:
                with open(path, "wb") as f:
                    if kind == "evtx":
                        evtx.write(f, size)
                    elif kind == "dump":
                        d = dumps[0] if size > len(dumps[0]) else dumps[1]
                        f.write(d[:size])
                        filler(f, max(0, size - len(d)), r)
                    elif kind == "opaque":
                        opaque_file(f, size, r)
                    elif kind in ("stamped", "partly", "plain", "blank"):
                        text_file(f, row, r)
                    elif kind == "xml":
                        xml_file(f, row, r)
                    elif kind == "bmp":
                        bmp_file(f, size, r)
                    elif kind == "jpeg":
                        signed_file(f, size, b"\xff\xd8\xff\xe0\x00\x10JFIF\x00", r)
                    elif kind == "png":
                        signed_file(f, size, b"\x89PNG\r\n\x1a\n", r)
                    elif kind == "pe":
                        signed_file(f, size, b"MZ", r)
                    elif kind == "pol":
                        signed_file(f, size, b"PReg\x01\x00\x00\x00", r)
            os.utime(path, (MTIME, MTIME))
            files += 1
            total += os.path.getsize(path)
    print(f"files {files} bytes {total}")


if __name__ == "__main__":
    main(*sys.argv[1:6])
