"""Builds the sanitized fixtures next to it from this Mac's own kernel lists (macOS 27.2, build 26B123).

Only the SHAPE is taken from the machine: one real group (inpcb, socket, two buffers, stats, tcpcb) per protocol is
used as a template, and every address, port, pid and uid is overwritten with an invented value (documentation ranges
192.0.2.0/24, 198.51.100.0/24, 2001:db8::/32). Nothing from the machine's real sockets is kept (ER-14).
Run: python3 -I make.py  (needs a binary the kernel shows all sockets to; /usr/bin/python3 is one).
"""
import ctypes, struct, socket

libc = ctypes.CDLL(None)


def get(name):
    n = ctypes.c_size_t(0)
    libc.sysctlbyname(name.encode(), None, ctypes.byref(n), None, 0)
    buf = ctypes.create_string_buffer(n.value + 65536)
    n = ctypes.c_size_t(len(buf))
    assert libc.sysctlbyname(name.encode(), buf, ctypes.byref(n), None, 0) == 0
    return buf.raw[: n.value]


def groups(data, tcp):
    """Splits a list into groups of records (each a bytearray), padded as the kernel pads them."""
    off, out, cur = 24, [], []
    while off + 8 <= len(data):
        ln, kind = struct.unpack_from("<II", data, off)
        if ln <= 24 and kind not in (1, 0x10):
            break
        cur.append(bytearray(data[off : off + ((ln + 7) & ~7)]))
        if kind == (0x20 if tcp else 0x8):
            out.append(cur)
            cur = []
        off += (ln + 7) & ~7
    return out


def v4(a):
    return b"\0" * 12 + socket.inet_aton(a)


def v6(a):
    return socket.inet_pton(socket.AF_INET6, a)


def make(tmpl, vflag, laddr, lport, faddr, fport, pid, epid, state=None, proto=6, zero_socket=False):
    g = [bytearray(r) for r in tmpl]
    inp, so = g[0], g[1]
    assert struct.unpack_from("<I", inp, 4)[0] == 0x10 and struct.unpack_from("<I", so, 4)[0] == 1
    struct.pack_into(">HH", inp, 16, fport, lport)
    inp[44] = vflag
    inp[48:64] = faddr
    inp[64:80] = laddr
    struct.pack_into("<q", inp, 28, 1000)  # inp_gencnt, below the header's generation
    if zero_socket:
        for i in range(8, len(so)):
            so[i] = 0
    else:
        struct.pack_into("<i", so, 36, proto)
        struct.pack_into("<I", so, 64, 501)
        struct.pack_into("<ii", so, 68, pid, epid)
    if state is not None:
        struct.pack_into("<i", g[-1], 36, state)
    return g


def unknown_record():
    return bytearray(struct.pack("<II8s", 16, 0x40, b"\0" * 8))  # XSO_KCREG, skipped by length


def build(gs, count):
    head = struct.pack("<IIQQ", 24, count, 5000, 77)
    return bytes(head + b"".join(bytes(r) for g in gs for r in g) + head)


tcp_t = groups(get("net.inet.tcp.pcblist_n"), True)
tcp_t = next(g for g in tcp_t if struct.unpack_from("<i", g[1], 36)[0] == 6 and len(g) == 6)
udp_t = next(g for g in groups(get("net.inet.udp.pcblist_n"), False) if struct.unpack_from("<i", g[1], 36)[0] == 17)
Z4, Z6 = v4("0.0.0.0"), v6("::")
tcp = [
    make(tcp_t, 1, v4("192.0.2.10"), 49152, v4("198.51.100.7"), 443, 4242, 4242, state=4),
    make(tcp_t, 1, v4("127.0.0.1"), 8080, Z4, 0, 4243, 4243, state=1),
    make(tcp_t, 2, Z6, 5000, Z6, 0, 4244, 4244, state=1),
    make(tcp_t, 2, v6("2001:db8::1"), 50000, v6("2001:db8::2"), 443, 4245, 4246, state=4),  # delegated: epid differs
    make(tcp_t, 1, v4("192.0.2.10"), 49153, v4("198.51.100.8"), 80, 0, 0, state=10, zero_socket=True),  # TIME_WAIT
    make(tcp_t, 1, v4("192.0.2.10"), 49154, v4("198.51.100.9"), 80, 4247, 4247, state=5),  # CLOSE_WAIT
    make(tcp_t, 1, v4("192.0.2.10"), 49155, v4("198.51.100.9"), 81, 4248, 4248, state=9),  # FIN_WAIT_2
]
tcp[2].insert(2, unknown_record())  # a record of a kind this reader does not know, between known ones
udp = [
    make(udp_t, 1, Z4, 5353, Z4, 0, 4250, 4250, proto=17),
    make(udp_t, 2, Z6, 5353, Z6, 0, 4251, 4251, proto=17),
    make(udp_t, 1, v4("192.0.2.10"), 50000, v4("198.51.100.9"), 53, 4252, 4252, proto=17),
]
open("tcp.bin", "wb").write(build(tcp, len(tcp)))
open("udp.bin", "wb").write(build(udp, len(udp)))
print("tcp", len(tcp), "udp", len(udp))
