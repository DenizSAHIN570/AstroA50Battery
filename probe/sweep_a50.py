#!/usr/bin/env python3
"""Small A50 Gen5 command sweep: 02 <cmd> + empty payload, cmd in [LO, HI].

Stops at the first reply that is neither the UNHANDLED_PACKET error nor all
zeros, and aborts if the device stops accepting writes (re-enumeration).
"""
import os, select, sys, time

DEV, LO, HI = sys.argv[1], int(sys.argv[2], 16), int(sys.argv[3], 16)
fd = os.open(DEV, os.O_RDWR | os.O_NONBLOCK)


def drain(wait):
    out, end = [], time.time() + wait
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], max(0, end - time.time()))
        if not r:
            break
        try:
            out.append(os.read(fd, 128))
        except BlockingIOError:
            break
    return out


drain(0.2)
for cmd in range(LO, HI + 1):
    p = bytes([0x02, cmd]) + bytes(62)
    try:
        os.write(fd, p)
    except OSError as e:
        print(f"cmd {cmd:02x}: write failed ({e}) -- device gone? ABORT")
        sys.exit(1)
    replies = drain(0.4)
    if not replies:
        print(f"cmd {cmd:02x}: no reply")
        continue
    for r in replies:
        if b"UNHANDLED_PACKET" in r:
            print(f"cmd {cmd:02x}: unhandled")
        elif not any(r[1:]):
            print(f"cmd {cmd:02x}: zeros")
        elif r[1] == 0x02 and not any(r[2:]):
            print(f"cmd {cmd:02x}: ACK, empty payload")
        else:
            print(f"cmd {cmd:02x}: *** RX [{len(r)}] {r.hex(' ')}")
    time.sleep(0.05)
print("sweep done, nothing interesting")
