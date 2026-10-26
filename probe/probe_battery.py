#!/usr/bin/env python3
"""Read-only A50 Gen5 battery check.

Sends the one battery query G HUB sends (HID++ 2.0 feature index 6, function 0), prints the
decoded reply, then keeps listening for pushed battery/state events. The base station
re-enumerates on USB (seen when the headset is powered off/docked), so the node is rediscovered
and the query repeated whenever the read fails.

Usage: probe_battery.py [listen_seconds]   (default 30)
Run it with `python3 -I`. Only this single query is ever written.
"""
import glob, os, select, sys, time

LISTEN = float(sys.argv[1]) if len(sys.argv) > 1 else 30.0

BATTERY_IDX = 0x06
STATE_IDX = 0x12  # 00 = headset on, 05 = off (low confidence)
T0 = time.time()


def log(msg):
    print(f"{time.time() - T0:7.2f}s {msg}", flush=True)


def find_node():
    """hidraw node of 046d:0b1c interface 8 (the letter shifts, so match via sysfs)."""
    for d in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        p = os.path.realpath(d + "/device")
        if ":1.8/" in p + "/" and "046D:0B1C" in p.upper():
            return "/dev/" + os.path.basename(d)
    return None


def describe(p):
    # 02 0c <len> 00 <idx> <fn|sw> <payload...>
    if len(p) < 7 or p[0] != 0x02 or p[1] != 0x0C:
        return f"?? {p[:16].hex(' ')}"
    idx, fn, pl = p[4], p[5], p[6:3 + p[2]]
    kind = "event" if fn & 0x0F == 0 else "reply"
    if idx == BATTERY_IDX and len(pl) >= 3:
        b0, b1, b2 = pl[:3]
        return (f"{kind} battery: shown={b1}%  raw?={b0}  "
                f"charging={'yes' if b2 else 'no'}  [{pl[:3].hex(' ')}]")
    if idx == STATE_IDX and pl:
        return f"{kind} state idx 0x12 = {pl[0]:02x} ({'off?' if pl[0] == 5 else 'on?'})"
    return f"{kind} idx {idx:02x} fn {fn:02x} [{pl.hex(' ')}]"


def session(path, end):
    """Query once, then print events until `end` or the node dies. Returns True if it died."""
    fd = os.open(path, os.O_RDWR | os.O_NONBLOCK)
    try:
        query = bytes([0x02, 0x0C, 0x03, 0x00, BATTERY_IDX, 0x0D]).ljust(64, b"\0")
        log(f"{path}: TX {query[:6].hex(' ')}")
        os.write(fd, query)
        while time.time() < end:
            r, _, _ = select.select([fd], [], [], max(0, min(1.0, end - time.time())))
            if r:
                log(describe(os.read(fd, 128)))
        return False
    except OSError as e:
        log(f"{path}: read/write failed: {e!r}  -> device re-enumerated?")
        return True
    finally:
        os.close(fd)


end = T0 + LISTEN
while time.time() < end:
    node = find_node()
    if node is None:
        log("no A50 interface-8 node, waiting...")
        time.sleep(0.5)
        continue
    if session(node, end):
        time.sleep(0.3)  # let the new node settle before rediscovery
log("done")
