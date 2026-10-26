#!/usr/bin/env python3
"""Read-only A50 Gen5 battery check.

Sends the one battery query G HUB sends (HID++ 2.0 feature index 6, function 0), prints the
decoded reply, then keeps listening for pushed battery/state events.

Usage: probe_battery.py /dev/hidrawN [listen_seconds]   (default 30; 0 = query only)
Run it with `python3 -I`. Only this single query is ever written.
"""
import os, select, sys, time

DEV = sys.argv[1]
LISTEN = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0

BATTERY_IDX = 0x06
STATE_IDX = 0x12  # 00 = headset on, 05 = off (low confidence)

fd = os.open(DEV, os.O_RDWR | os.O_NONBLOCK)


def read_until(deadline):
    while time.time() < deadline:
        r, _, _ = select.select([fd], [], [], max(0, deadline - time.time()))
        if not r:
            return
        try:
            yield os.read(fd, 128)
        except BlockingIOError:
            return


def describe(p, t0):
    stamp = f"{time.time() - t0:6.2f}s"
    # 02 0c <len> 00 <idx> <fn|sw> <payload...>
    if len(p) < 7 or p[0] != 0x02 or p[1] != 0x0C:
        return f"{stamp} ?? {p[:16].hex(' ')}"
    idx, fn, pl = p[4], p[5], p[6:3 + p[2]]
    kind = "event" if fn & 0x0F == 0 else "reply"
    if idx == BATTERY_IDX and len(pl) >= 3:
        b0, b1, b2 = pl[:3]
        return (f"{stamp} {kind} battery: shown={b1}%  raw?={b0}  "
                f"charging={'yes' if b2 else 'no'}  [{pl[:3].hex(' ')}]")
    if idx == STATE_IDX and pl:
        return f"{stamp} {kind} state idx 0x12 = {pl[0]:02x} ({'off?' if pl[0] == 5 else 'on?'})"
    return f"{stamp} {kind} idx {idx:02x} fn {fn:02x} [{pl.hex(' ')}]"


t0 = time.time()
for _ in read_until(time.time() + 0.2):  # discard anything pending
    pass

query = bytes([0x02, 0x0C, 0x03, 0x00, BATTERY_IDX, 0x0D]).ljust(64, b"\0")
print(f"TX {query[:8].hex(' ')} ...")
os.write(fd, query)

got_reply = False
for p in read_until(time.time() + 1.0):
    print(describe(p, t0))
    got_reply = got_reply or (p[4:6] == bytes([BATTERY_IDX, 0x0D]))
if not got_reply:
    print("no battery reply (wrong hidraw node, or framing/index differs on this unit)")

if LISTEN > 0:
    print(f"listening {LISTEN:.0f}s for pushed events (plug/unplug the headset, power-cycle it)...")
    end = time.time() + LISTEN
    while time.time() < end:
        for p in read_until(end):
            print(describe(p, t0))
os.close(fd)
