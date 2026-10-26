#!/usr/bin/env python3
"""Cautious A50 Gen5 probe: send query-shaped output reports, log any replies.

Usage: probe_a50.py /dev/hidrawN
Output report layout (from the report descriptor): ID 0x02 + 63 bytes.
"""
import os, select, sys, time

DEV = sys.argv[1]
fd = os.open(DEV, os.O_RDWR | os.O_NONBLOCK)


def pkt(*head):
    b = bytes(head)
    return b + bytes(64 - len(b))


# (label, packet) -- all read/query-shaped, nothing that looks like a setter
CANDIDATES = [
    ("all-zero payload", pkt(0x02)),
    ("cmd 0x01", pkt(0x02, 0x01)),
    ("cmd 0x02", pkt(0x02, 0x02)),
    ("cmd 0x03", pkt(0x02, 0x03)),
    ("HID++2 root ping idx=ff", pkt(0x02, 0xFF, 0x00, 0x1A, 0x00, 0x00, 0xAA)),
    ("HID++2 root ping idx=00", pkt(0x02, 0x00, 0x00, 0x1A, 0x00, 0x00, 0xAA)),
    ("HID++2 getFeature 0x1000 (battery status)", pkt(0x02, 0xFF, 0x00, 0x0A, 0x10, 0x00)),
    ("HID++2 getFeature 0x1004 (unified battery)", pkt(0x02, 0xFF, 0x00, 0x0A, 0x10, 0x04)),
    ("HID++2 getFeature 0x1F20 (adc measurement)", pkt(0x02, 0xFF, 0x00, 0x0A, 0x1F, 0x20)),
]


def drain(wait):
    out = []
    end = time.time() + wait
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], max(0, end - time.time()))
        if not r:
            break
        try:
            out.append(os.read(fd, 128))
        except BlockingIOError:
            break
    return out


drain(0.2)  # discard anything pending
for label, p in CANDIDATES:
    print(f"\n>>> {label}\n    TX {p[:12].hex(' ')} ...")
    try:
        os.write(fd, p)
    except OSError as e:
        print(f"    write failed: {e}")
        continue
    replies = drain(0.6)
    if not replies:
        print("    (no reply)")
    for r in replies:
        print(f"    RX [{len(r)}] {r.hex(' ')}")
os.close(fd)
