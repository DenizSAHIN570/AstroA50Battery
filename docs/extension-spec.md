# GNOME extension spec — Astro A50 Gen 5

Status: draft for approval. Nothing here is built yet. Items marked **(guess)** are my calls, not
decisions; they need a yes or a correction before the build.

## Intent

Show the A50's state at a glance in the GNOME top bar and in a click-open popup, the same way
`../AsusMouseBattery/gnome-extension` does for the Asus mouse. The headset is the "fuller" device:
besides battery it has a volume wheel and a game/voice balance, and the user wants those visible too.

## Goals

- A **simple battery icon** in the top bar, with the **game/voice balance shown beside it**.
- A popup, styled like the Asus one (a plain `PopupMenu`), showing:
  **device name, battery info, volume level, volume balance.**
- Live: updates immediately when the daemon's DBus properties change (no polling in the extension).
- Hides itself when the daemon is not running or the headset is absent.

## Non-goals

- No controls. The extension is read-only: it cannot set volume or balance (the daemon only reads),
  and the daemon must never send setters.
- No time-to-empty/full or voltage: the daemon reports 0 for those; do not show rows for them.
- No notifications (low battery etc.) in this version.
- No preferences UI.

## Data source (daemon contract)

Session bus, `com.mousewatch.AstroBattery`, object `/com/mousewatch/AstroBattery/device0`,
interface `com.mousewatch.Battery1` (the Asus interface name, so shared code stays possible).

| Property | Type | Meaning |
|---|---|---|
| `DeviceName` | s | "Astro A50" |
| `Percentage` | y | Battery, steps of 5 (0, 5, … 100). Don't imply finer precision. |
| `Status` | s | `charging` / `discharging` / `fully-charged` / `unknown` |
| `IsPresent` | b | false while the base station is re-enumerating or unplugged |
| `IsActive` | b | false while the headset is powered off or docked |
| `Volume` | y | 0–100 (wheel position 0–31 scaled). Derived by the daemon, not yet checked against a real display. |
| `Balance` | y | 0–12; 0 = voice maxed, 6 = middle, 12 = game maxed |
| `DaemonVersion` | s | for the popup footer |

`TimeToFull`, `TimeToEmpty`, `VoltageMv` exist for interface parity and are always 0.

## Top bar

`[battery icon] [NN%] [balance indicator]` in one `PanelMenu.Button` / `St.BoxLayout`, like the Asus button.

**Battery icon** (reuse the Asus rules): icon by percentage (`battery-full` > 50, `battery-good` > 10,
else `battery-caution`; `battery-full-charged` when fully charged), colour classes
green / orange / red from the same thresholds. **(guess)** while charging, use the `-charging`
symbolic variant so charge state is visible without opening the popup.

**Balance indicator (guess):** a tiny horizontal bar (about 28 px wide) with a marker at
`Balance / 12`, a centre tick at the middle, a **V** hint on the left end and a **G** on the right.
Marker sits dead centre at `Balance = 6`. It is monochrome and follows the panel foreground colour,
so it doesn't compete with the battery colour. Alternative if the bar is too fiddly: plain text such
as `V3` / `G2` / `=`, hidden at centre.

**Percentage text:** shown next to the icon (decision 2 below; the draft said none).
The exact number is in the popup. (The Asus button shows `NN%`; say if you want that here too.)

States:

| Condition | Top bar |
|---|---|
| daemon not on the bus, or `IsPresent` false | hidden (same as Asus) |
| `IsActive` false (off or docked) | icon and balance dimmed (opacity ~50%); popup says "Off / docked" |
| normal | as above |

## Popup

Plain `PopupMenu`, non-interactive rows, in this order:

1. Device name (`DeviceName`), as the Asus header row.
2. Separator.
3. **Battery:** `Battery: 35%  ·  Charging` (status labels as in the Asus `_formatStatus`;
   when inactive append `(headset off or docked)`).
4. **Volume:** `Volume: 74%`.
5. **Balance:** a text line such as `Balance: Voice +3`, `Balance: Game +4`, `Balance: Centred`,
   plus a wider version of the top-bar bar **(guess)**. Mapping: `d = Balance - 6`;
   `d < 0` → `Voice +|d|`, `d > 0` → `Game +d`, `d = 0` → `Centred`.
6. Separator.
7. Footer (insensitive): `Daemon: vX  ·  Extension: vY`, as in the Asus extension.

## Behaviour and structure

Mirror the Asus extension's structure (`extension.js`, `metadata.json`, `stylesheet.css`):

- `Gio.DBusProxy.new_for_bus` with `DO_NOT_AUTO_START`, created asynchronously; bail out if the
  extension was disabled before the callback.
- Connect `g-properties-changed` and `notify::g-name-owner`; both call one `_update()`.
- `_update()` reads cached properties with defaults, wraps the body in try/catch and logs with a
  `[astro-a50-battery]` prefix.
- `disable()` disconnects handlers, clears the proxy and destroys the indicator.
- Pure formatting helpers (`_balanceLabel`, `_formatStatus`) live in a small module and get a
  `test-format.js` like the Asus one.

Identity **(guess)**: UUID `astro-a50-battery@gnome`, name "Astro A50 Battery", shell versions
45–50 (same as Asus). Installed to `~/.local/share/gnome-shell/extensions/<uuid>/` by the repo's
`install.sh` (extension copy step to be added there); enabling is still the user's
`gnome-extensions enable`, and on Wayland a re-login is needed to load a new extension.

## Both devices on one bar

The Asus extension and this one are independent indicators, each hiding when its daemon is absent.
No shared state. If a combined indicator is wanted later, both daemons already speak the same
`Battery1` interface, so that is a separate change.

## Acceptance (how we'll know it works)

1. With the daemon running and the headset on: icon + balance bar in the top bar; popup shows the
   five items above with values matching `busctl --user get-property …`.
2. Turn the volume wheel / press a balance button: popup and bar update within about a second
   without reopening anything.
3. Plug the cable: icon switches to the charging variant; unplug: back.
4. Power the headset off: dimmed; on: normal.
5. Stop the daemon (or trigger a base-station re-enumeration): indicator hides, then returns by itself.
6. `gnome-shell` journal has no errors with the `[astro-a50-battery]` prefix.

## Decisions (2026-10-26)

1. Balance indicator: tiny bar with marker. **Approved.**
2. Top bar shows the icon **and `NN%`** (changed from the draft's "icon only"). **Approved.**
3. Charging variant of the battery icon in the top bar. **Approved.**
4. `Volume` = wheel position / 31. Evidence so far: Windows capture had position 19 while G HUB
   showed 61% (19/31 = 61.3%), max position 31 = 100. A 100 -> 0 wheel recording is available if the
   endpoints ever look wrong.

## Build status

Implemented in `gnome-extension/` (`extension.js`, `format.js`, `metadata.json`, `stylesheet.css`,
`test-format.js`). `gjs -m gnome-extension/test-format.js`: 23/23 pass. Not yet loaded in a real
Shell session (needs install + re-login on Wayland); acceptance checklist above still to run.
