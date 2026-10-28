"""Synthetic large log folder in the shape of /samples (invented data, never user logs).

usage: python3 -I gen_large.py OUT_DIR SCALE
SCALE 1.0 is about 7.6 GB: large log4net-pattern text files with stack traces, CLEF files, many small
serilog-text files and a few gzip-compressed text files. Deterministic (fixed seeds).
"""
import gzip
import json
import os
import random
import sys
from datetime import datetime, timedelta, timezone

OUT = sys.argv[1]
SCALE = float(sys.argv[2]) if len(sys.argv) > 2 else 1.0
MB = 1 << 20

HOSTS = [f"WEB{i:02d}" for i in range(1, 13)] + ["BATCH01", "GW01", "SQL01"]
LOGGERS = ["Contoso.Orders.Intake", "Contoso.Orders.Payments", "Contoso.Orders.Cache", "Contoso.Orders.Sql",
           "Contoso.Orders.Http", "Contoso.Billing.Export", "Contoso.Auth.Tokens", "Contoso.Orders.Program"]
LEVELS = [("DEBUG", 30), ("INFO", 55), ("WARN", 10), ("ERROR", 5)]
LEVEL_POOL = [l for l, w in LEVELS for _ in range(w)]
CLEF_LEVEL = {"DEBUG": "Debug", "INFO": "Information", "WARN": "Warning", "ERROR": "Error"}
SER_LEVEL = {"DEBUG": "DBG", "INFO": "INF", "WARN": "WRN", "ERROR": "ERR"}
TEMPLATES = [
    ("Order {OrderNo} accepted for customer {CustomerId} total {Total}", lambda r: {"OrderNo": r.randint(10000, 99999), "CustomerId": f"C-{r.randint(1000, 99999)}", "Total": r.randint(100, 90000)}),
    ("Slow response from {Endpoint}: {ElapsedMs} ms", lambda r: {"Endpoint": r.choice(["/v2/captures", "/v2/refunds", "/api/orders", "/api/stock"]), "ElapsedMs": r.randint(50, 9000)}),
    ("Tariff cache refreshed with {Entries} entries, hit ratio {HitRatio}", lambda r: {"Entries": r.randint(100, 900), "HitRatio": round(r.random(), 2)}),
    ("SQL command {Command} took {ElapsedMs} ms on {Database}", lambda r: {"Command": r.choice(["GetOrder", "SaveOrder", "ListStock", "Reserve"]), "ElapsedMs": r.randint(1, 4000), "Database": "orders"}),
    ("User {UserId} signed in from {ClientIp}", lambda r: {"UserId": f"u{r.randint(1, 50000)}", "ClientIp": f"10.{r.randint(0, 255)}.{r.randint(0, 255)}.{r.randint(1, 254)}"}),
    ("Payment capture {CaptureId} failed: {Reason}", lambda r: {"CaptureId": f"cap_{r.getrandbits(48):012x}", "Reason": r.choice(["timeout", "declined", "gateway 502"])}),
    ("Request {Method} {Path} returned {StatusCode} in {ElapsedMs} ms", lambda r: {"Method": r.choice(["GET", "POST", "PUT"]), "Path": r.choice(["/api/orders", "/api/cart", "/health", "/api/stock/42"]), "StatusCode": r.choice([200, 200, 200, 201, 404, 500]), "ElapsedMs": r.randint(1, 3000)}),
    ("Heartbeat from worker {Worker} queue depth {Depth}", lambda r: {"Worker": f"w{r.randint(1, 32)}", "Depth": r.randint(0, 5000)}),
]
STACK = (
    "System.TimeoutException: The operation has timed out.\n"
    "   at Contoso.Orders.Payments.CaptureClient.SendAsync(CaptureRequest request) in C:\\src\\Payments\\CaptureClient.cs:line 88\n"
    "   at Contoso.Orders.Payments.PaymentService.CaptureAsync(Order order) in C:\\src\\Payments\\PaymentService.cs:line 141\n"
    "   at Contoso.Orders.Intake.OrderPipeline.RunAsync(Order order) in C:\\src\\Intake\\OrderPipeline.cs:line 57"
)
START = datetime(2026, 10, 14, 0, 0, 0, tzinfo=timezone.utc)


def render(tpl, args):
    out = tpl
    for k, v in args.items():
        out = out.replace("{" + k + "}", str(v))
    return out


def events(seed):
    r = random.Random(seed)
    t = START + timedelta(seconds=r.randint(0, 3600))
    while True:
        t += timedelta(microseconds=r.randint(200, 40000))
        lvl = r.choice(LEVEL_POOL)
        tpl, gen = r.choice(TEMPLATES)
        args = gen(r)
        yield t, lvl, r.choice(LOGGERS), r.randint(1, 64), tpl, args, (lvl == "ERROR" and r.random() < 0.5)


def write_until(f, size, lines):
    written = 0
    buf = []
    for line in lines:
        b = line.encode("utf-8")
        buf.append(b)
        written += len(b)
        if len(buf) >= 4096:
            f.write(b"".join(buf))
            buf.clear()
        if written >= size:
            break
    f.write(b"".join(buf))
    return written


def log4net_lines(seed, host):
    for t, lvl, logger, th, tpl, args, exc in events(seed):
        msg = render(tpl, args)
        line = f"{t:%Y-%m-%d %H:%M:%S},{t.microsecond // 1000:03d} [{th}] {lvl:<5} {logger} - {msg} host={host}\n"
        if exc:
            line += STACK + "\n"
        yield line


def serilog_text_lines(seed):
    for t, lvl, logger, th, tpl, args, exc in events(seed):
        local = t.astimezone(timezone(timedelta(hours=9)))
        line = f"{local:%Y-%m-%d %H:%M:%S}.{local.microsecond // 1000:03d} +09:00 [{SER_LEVEL[lvl]}] {render(tpl, args)}\n"
        if exc:
            line += STACK + "\n"
        yield line


def clef_lines(seed, host):
    for t, lvl, logger, th, tpl, args, exc in events(seed):
        o = {"@t": t.strftime("%Y-%m-%dT%H:%M:%S.%f") + "0Z", "@mt": tpl}
        if lvl != "INFO":
            o["@l"] = CLEF_LEVEL[lvl]
        o.update(args)
        o["SourceContext"] = logger
        o["MachineName"] = host
        o["ThreadId"] = th
        if exc:
            o["@x"] = STACK
        yield json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n"


def main():
    total = 0
    plan = []
    # 4.4 GB: 8 large log4net-pattern files.
    for i in range(8):
        plan.append((f"web/{HOSTS[i]}/orders-{HOSTS[i].lower()}.log", int(560 * MB * SCALE), "log4net", 100 + i, HOSTS[i]))
    # 2.3 GB: 6 CLEF files.
    for i in range(6):
        h = HOSTS[8 + i % 4]
        plan.append((f"clef/{h}/requests-{i}.clef", int(390 * MB * SCALE), "clef", 200 + i, h))
    # 0.5 GB: 600 small serilog-text files of 0.8 MB.
    for i in range(int(600 * SCALE) or 1):
        plan.append((f"small/day{i // 100:02d}/svc-{i:04d}.txt", int(0.8 * MB), "serilog", 1000 + i, None))
    # 0.4 GB of text in 4 gzip files.
    for i in range(4):
        plan.append((f"compressed/batch-{i}.log.gz", int(100 * MB * SCALE), "gz", 300 + i, "BATCH01"))
    for rel, size, kind, seed, host in plan:
        path = os.path.join(OUT, rel)
        if os.path.exists(path):
            total += size
            continue
        os.makedirs(os.path.dirname(path), exist_ok=True)
        tmp = path + ".part"
        if kind == "gz":
            with gzip.open(tmp, "wb", compresslevel=1) as f:
                write_until(f, size, log4net_lines(seed, host))
        else:
            gen = {"log4net": lambda: log4net_lines(seed, host), "clef": lambda: clef_lines(seed, host), "serilog": lambda: serilog_text_lines(seed)}[kind]()
            with open(tmp, "wb") as f:
                write_until(f, size, gen)
        os.replace(tmp, path)
        total += size
        print(f"{rel} {size / MB:.0f} MB", flush=True)
    print(f"total about {total / 1e9:.2f} GB (uncompressed)")


main()
