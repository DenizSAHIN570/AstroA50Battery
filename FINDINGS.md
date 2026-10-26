# Astro A50 Gen 5 — battery protocol findings

Goal: read the headset battery on Linux and show it in the GNOME top bar, the same way
`../AsusMouseBattery` does for the Asus mice (Rust daemon -> DBus -> GNOME extension).

Status (2026-10-26): **battery protocol decoded from a Windows USBPcap capture of G HUB.**
Next: verify on Linux with a read-only probe, then build the daemon backend.
Raw captures and decoded packet lists are in `C:\a50-capture\` on the Windows machine
(`capture-0N-*.pcap`, `packets-0N.txt`, `notes.txt`).

## Device

| | |
|---|---|
| USB ID | `046d:0b1c` ("Logitech A50", Astro is Logitech-owned) |
| bcdDevice | 1.03 |
| Control channel | USB interface 8, HID class, EP 5 IN / EP 5 OUT, 64 bytes each |
| Linux node | `/dev/hidraw0` at the time (letter shifts; match by VID:PID + `:1.8` in the sysfs path) |
| Access | `uaccess` ACL already gives the session user rw, no udev rule needed to probe |
| Other interfaces | 0-7 are USB Audio (Voice, Mic, Game, Stream Out) |

HID report descriptor: vendor usage page `0xFF32`, usage `0x74`, **report ID 2**,
63-byte input and 63-byte output reports.

## Protocol: HID++ 2.0 wrapped in a `02 0c` envelope

Corrects the earlier notes: the Linux sweep sent `02 <cmd>`, which is not the real framing. The
`HID_ERROR_UNHANDLED_PACKET` replies were the firmware rejecting packets that lacked the `0c` envelope.

64-byte reports, zero padded, both directions:

```
02 0c <len> 00 <feature index> <function<<4 | swId> <params...>
```

- `02`: report ID. `0c`: envelope/type byte (constant in every captured packet).
- `<len>`: number of bytes after it (index + function byte + params/payload).
- `00`: constant.
- `<feature index>`: HID++ 2.0 feature index. Indexes are looked up per device through the root
  (index 0) and IFeatureSet (index 1) features; G HUB does this at startup (count was `0x15` = 21).
  **Don't hardcode index 6 in the daemon: enumerate and resolve it by feature ID.**
  (Which feature ID index 6 maps to is not yet read out of the capture; check `packets-01.txt` rows
  for `01 1d`, the IFeatureSet GetFeatureID calls.)
- Function byte: high nibble = function, low nibble = software ID. G HUB uses swId `0xd`
  (`0d`, `1d`, `2d`, ...). Events from the device use swId `0` (`00`).
- Replies echo `<index> <function byte>` then the payload. Error replies are in the old
  `HID_ERROR_UNHANDLED_PACKET` shape.

Sanity check against the known identity commands (all matched on Windows):
firmware version `04 1d` -> `01 03 00 18 04 'main'` (1.3.0.24), serial `04 2d` -> `0c '[redacted]'`,
name `05 2d` -> `03 'A50'`.

## Battery (index 6) — decoded

Query (G HUB sends it once at startup, twice in capture A), function 0:

```
OUT 02 0c 03 00 06 0d
IN  02 0c 06 00 06 0d  <b0> <b1> <b2>
```

The device also **pushes the same payload unsolicited** whenever the battery or charging state
changes (event form, swId 0): `85 02 0c 06 00 06 00 <b0> <b1> <b2>`. G HUB does not poll
(capture B: zero HID traffic in ~3.5 min of idle).

| Byte | Meaning | Confidence |
|---|---|---|
| `b1` | **Displayed battery percent** (what G HUB shows). Moves in steps of 5 (looks like `b0` rounded up to a multiple of 5). | high |
| `b0` | Finer raw percent (30, 31, 32 observed). Never shown by G HUB. | moderate (guess) |
| `b2` | **Charging flag**: `01` charging (cable plugged), `00` not. | high |

Evidence:

| Capture | Event | `b0 b1 b2` | G HUB showed |
|---|---|---|---|
| 01 startup | query reply | `1f 23 00` | 35% |
| 03 charging | cable plugged in, t=24.4 s | `1e 1e 01` | 30% |
| 03 charging | still charging, t=26.5 s | `1f 23 01` | 35% |
| 03 charging | cable unplugged, t=95.3 s | `20 23 00` | 35% |
| 04 power cycle | after power on, t=26.1 s | `20 23 00` | 35% |

Rows 2-4 are charging via the headset's own cable. Dock/cradle charging (capture 05) behaves the
same for the battery event:

| Capture | Event | `b0 b1 b2` | G HUB showed |
|---|---|---|---|
| 05 dock | headset put on dock, t=32.0 s | `1f 23 01` | 35% |
| 05 dock | headset taken off dock, t=72.1 s | `20 23 00` | 35% |

`b0` is not monotonic (`20` off-charge, `1f` on dock/startup, `1e` at plug-in), so treat it as a noisy raw
value and use `b1` for display.

## Power / connection events (low confidence)

From capture D (headset powered off ~t=15 s, on ~t=26 s), unsolicited events:

| t | Packet payload | Reading |
|---|---|---|
| 14.77 s | idx `0a` fn0: `06` | unchanged (query at startup also returns `06`) |
| 14.78 s | idx `12` fn0: `05` | headset powered off / disconnected (guess) |
| 16.32 s | idx `12` fn0: `05` | repeat |
| 25.98 s | idx `12` fn0: `00` | headset on / connected (guess; startup query `12 6d` also returns `00`) |
| 26.07 s | idx `06` battery event `20 23 00` | battery re-announced after reconnect |

Docking (capture 05) produces the same idx `12` = `05` event as powering off, so `05` means
"headset not active" (powered off **or** docked), `00` means active. Dock sequence, unsolicited:

| t | Packet | Reading |
|---|---|---|
| 27.65 s | idx `0c` fn0: `00 00 00 0a` | dock state changed (guess: `0a` docked; startup query read `0b` undocked) |
| 31.15 s | idx `0a` fn0: `06` | repeat of unchanged value |
| 31.16 s | idx `12` fn0: `05` | headset inactive |
| 31.97 s | idx `06` battery: `1f 23 01` | charging flag set |
| 70.43 s | idx `12` fn0: `00` | headset active again (taken off dock) |
| 72.09 s | idx `06` battery: `20 23 00` | charging flag cleared |
| 73.94 s | idx `08` fn0: `14 00 0e` | unknown |
| 75.11 s | idx `0c` fn0: `00 00 00 0b` | back to `0b` (undocked) |

No battery event is sent when the headset goes off, so the daemon should show the last value as
stale while idx `12` = `05`, and re-query battery on `00`. The battery reading while docked is still
valid (it charges).

## Volume wheel (index 8) — decoded (capture 06)

The headset wheel pushes unsolicited events `85 02 0c 06 00 08 00 <b0> 00 <b2>`, about one per
0.6 s while turning. Starting from 61 -> 100 -> 0 (G HUB value):

| t | `b0` | `b2` | Reading |
|---|---|---|---|
| 5.9 s | `17` (23) | `10` (16) | going up |
| 6.5 s | `1f` (31) | `15` (21) | max (100) |
| 14.8 s | `1c` (28) | `13` (19) | going down |
| 15.4 s | `11` (17) | `0c` (12) | |
| 16.0 s | `07` (7) | `05` (5) | |
| 16.6 s–17.8 s | `00` | `00` | min (0), repeated |

- `b0` is the wheel position on a **0–31** scale: percent ≈ `b0 / 31 * 100`. Cross-check: the
  startup query `08 0d` returned `13 00 0d` (b0 = 19 = 61%) while the volume was 61.
- `b2` ≈ `b0 * 0.68` (0–21 scale); meaning unknown, ignore it.
- Query form: `02 0c 03 00 08 0d` (same shape as the battery query). Not needed for the battery.
- This also explains the earlier idx `08` event `14 00 0e` after undocking: the wheel position.

## Other captured queries (meaning unknown, kept for later)

Startup also reads indexes `07`, `08`, `09`, `0a`, `0c`, `0d` (large config tables), `0e`, `0f`
(`64` = 100), `10`, `14`. Not needed for the battery.

## Linux verification (2026-10-26, read-only)

`probe/probe_battery.py` on `/dev/hidraw0` (interface 8): sent `02 0c 03 00 06 0d`, got
`02 0c 06 00 06 0d 1f 23 00` = **35%, not charging**, identical to the Windows capture. Framing and
battery decode confirmed on Linux. The user confirmed the headset was really at about 35%, so `b1`
is the true level, not just what G HUB displays.

IFeatureSet queries (`02 0c 03 00 01 0d` -> count `0x15`; `02 0c 04 00 01 1d <i>`) work, but the
`<i>` reply is **not** a single standard feature ID. It is a stream starting at entry `i`, header
`0e xx`, then 4-byte entries, running to the end of the table (reply length shrinks by 4 per `i`).
Raw stream for `i=0` (after the 2-byte header):

```
00 00 02 00 | 01 00 01 01 | 0a 00 06 01 | 02 00 01 01 | 00 00 06 01 | 01 00 02 01 | 04 00 04 01 |
08 00 03 02 | 00 00 04 06 | 04 00 02 06 | 09 00 02 03 | 03 00 04 08 | 00 00 01 02 | 0d 00 01 00 | ...
```

Entry layout not understood (entries 0 and 1 look like root `0x0000` / IFeatureSet `0x0001` in LE, but
later entries don't map to plausible IDs, so the format differs from stock HID++ 2.0). Not resolved;
the daemon should keep using index `6` / `0x12` as G HUB does, and verify with a live battery reply.
Re-extract the real G HUB calls from `capture-01-app-startup.pcap` if feature IDs become necessary.

## Prior art

- [eh-fifty](https://pypi.org/project/eh-fifty): reverse-engineered Python lib for **Gen 4** over PyUSB.
  Different device generation; HID++ 2.0 here is the Logitech-era protocol.
- No public Gen 5 / A50 X battery protocol found as of 2026-10-26.

## Open items

1. Resolve feature IDs for indexes 6 and 0x12 from the IFeatureSet replies in `capture-01-*.pcap`
   (pin the battery to a standard feature such as `0x1000`/`0x1004`, or confirm it is vendor-specific).
2. Confirm `b1` rounding with another discharge reading; confirm `b0` meaning.
3. Test "out of range" (headset on but base unreachable). Dock case is done (capture 05).
4. Verify on Linux with a read-only probe sending exactly `02 0c 03 00 06 0d` (+ zero pad to 64).
5. Build the daemon backend, reusing `../AsusMouseBattery/daemon/src/hid/` (discovery by VID:PID +
   interface 8, event-driven reads plus an initial query, DBus device object, GNOME extension entry).

## Files

- `probe/probe_a50.py`, `probe/sweep_a50.py` — the original Linux probes. They use the **wrong
  framing** (`02 <cmd>`); keep for reference, do not rely on their results.
