use anyhow::Result;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tokio::io::unix::AsyncFd;

pub const VID: u16 = 0x046D;
pub const PID: u16 = 0x0B1C;
/// The vendor control channel is USB interface 8; the others are audio.
const CONTROL_INTERFACE: &str = ":1.8/";
const REPORT_LEN: usize = 64;

// HID++ 2.0 feature indexes, as G HUB uses them on firmware 1.3.0.24.
const FIRMWARE_IDX: u8 = 0x04;
const NAME_IDX: u8 = 0x05;
const BATTERY_IDX: u8 = 0x06;
const VOLUME_IDX: u8 = 0x08;
const BALANCE_IDX: u8 = 0x0A;
const ACTIVE_IDX: u8 = 0x12;

/// Read-only queries G HUB sends at startup: `02 0c <len> 00 <idx> <fn<<4|swId>`.
pub const QUERIES: [[u8; 6]; 6] = [
    [0x02, 0x0C, 0x03, 0x00, BATTERY_IDX, 0x0D],
    [0x02, 0x0C, 0x03, 0x00, VOLUME_IDX, 0x0D],
    [0x02, 0x0C, 0x03, 0x00, BALANCE_IDX, 0x0D],
    [0x02, 0x0C, 0x03, 0x00, ACTIVE_IDX, 0x6D],
    [0x02, 0x0C, 0x03, 0x00, FIRMWARE_IDX, 0x1D],
    [0x02, 0x0C, 0x03, 0x00, NAME_IDX, 0x2D],
];

#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// `percent` is the value G HUB shows (steps of 5).
    Battery { percent: u8, charging: bool },
    /// Raw wheel position, 0..=31.
    Volume(u8),
    /// Game/voice balance, 0 = voice maxed, 6 = middle, 12 = game maxed.
    Balance(u8),
    /// false = headset powered off or docked.
    Active(bool),
    /// Firmware version as `major.minor.patch.build`.
    Firmware(String),
    /// Product name as the headset reports it ("A50").
    Name(String),
}

/// Decode one 64-byte input report; replies and pushed events share a shape.
pub fn parse_report(p: &[u8]) -> Option<Event> {
    // 02 0c <len> 00 <idx> <fn|sw> <payload...>
    if p.len() < 7 || p[0] != 0x02 || p[1] != 0x0C {
        return None;
    }
    let end = (3 + p[2] as usize).min(p.len());
    let payload = p.get(6..end)?;
    match p[4] {
        BATTERY_IDX if payload.len() >= 3 => Some(Event::Battery {
            percent: payload[1].min(100),
            charging: payload[2] != 0,
        }),
        VOLUME_IDX if !payload.is_empty() => Some(Event::Volume(payload[0].min(31))),
        BALANCE_IDX if !payload.is_empty() => Some(Event::Balance(payload[0].min(12))),
        ACTIVE_IDX if !payload.is_empty() => Some(Event::Active(payload[0] == 0)),
        // Function 1 of the firmware feature: `01 03 00 18 04 'main'` = 1.3.0.24.
        FIRMWARE_IDX if p[5] >> 4 == 1 && payload.len() >= 4 => Some(Event::Firmware(format!(
            "{}.{}.{}.{}",
            payload[0], payload[1], payload[2], payload[3]
        ))),
        // Function 2 of the name feature: length byte, then ASCII.
        NAME_IDX if p[5] >> 4 == 2 && !payload.is_empty() => {
            let len = (payload[0] as usize).min(payload.len() - 1);
            let name = String::from_utf8_lossy(&payload[1..1 + len]).trim().to_string();
            (!name.is_empty()).then_some(Event::Name(name))
        }
        _ => None,
    }
}

/// hidraw node for interface 8 of the A50, found through sysfs because the
/// letter shifts and the base station re-enumerates.
pub fn find_node() -> Option<PathBuf> {
    let base = Path::new("/sys/class/hidraw");
    let mut entries: Vec<_> = fs::read_dir(base).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(uevent) = fs::read_to_string(base.join(&name).join("device/uevent")) else {
            continue;
        };
        // HID_ID=0003:0000046D:00000B1C
        let Some(line) = uevent.lines().find(|l| l.starts_with("HID_ID=")) else {
            continue;
        };
        let parts: Vec<&str> = line.trim_start_matches("HID_ID=").split(':').collect();
        if parts.len() != 3 {
            continue;
        }
        let ids = (u32::from_str_radix(parts[1], 16), u32::from_str_radix(parts[2], 16));
        if ids != (Ok(VID as u32), Ok(PID as u32)) {
            continue;
        }
        let Ok(resolved) = fs::canonicalize(base.join(&name)) else {
            continue;
        };
        if resolved.to_string_lossy().contains(CONTROL_INTERFACE) {
            return Some(PathBuf::from("/dev").join(name));
        }
    }
    None
}

pub struct Headset {
    fd: AsyncFd<File>,
}

impl Headset {
    pub fn open(path: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)?;
        // SAFETY: `file` owns a valid open fd that stays open for the AsyncFd's lifetime.
        let fd = unsafe { AsyncFd::register(file) }
            .map_err(|e| anyhow::anyhow!("registering hidraw fd: {e}"))?;
        Ok(Self { fd })
    }

    pub async fn write(&self, data: &[u8]) -> io::Result<()> {
        let mut buf = [0u8; REPORT_LEN];
        buf[..data.len()].copy_from_slice(data);
        loop {
            let mut guard = self.fd.writable().await?;
            match guard.try_io(|fd| {
                let mut f: &File = fd.get_ref();
                f.write(&buf)
            }) {
                Ok(res) => return res.map(|_| ()),
                Err(_would_block) => continue,
            }
        }
    }

    /// Next input report. Errors (EIO/ENODEV) mean the node went away.
    pub async fn read(&self) -> io::Result<Vec<u8>> {
        loop {
            let mut guard = self.fd.readable().await?;
            let mut buf = [0u8; REPORT_LEN * 2];
            match guard.try_io(|fd| {
                let mut f: &File = fd.get_ref();
                f.read(&mut buf)
            }) {
                Ok(Ok(0)) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(Ok(n)) => return Ok(buf[..n].to_vec()),
                Ok(Err(e)) => return Err(e),
                Err(_would_block) => continue,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pad(head: &[u8]) -> Vec<u8> {
        let mut v = head.to_vec();
        v.resize(REPORT_LEN, 0);
        v
    }

    #[test]
    fn battery_reply_matches_live_capture() {
        // 35%, not charging (live reply)
        let p = pad(&[0x02, 0x0C, 0x06, 0x00, 0x06, 0x0D, 0x1F, 0x23, 0x00]);
        assert_eq!(parse_report(&p), Some(Event::Battery { percent: 35, charging: false }));
    }

    #[test]
    fn battery_event_charging() {
        let p = pad(&[0x02, 0x0C, 0x06, 0x00, 0x06, 0x00, 0x1F, 0x23, 0x01]);
        assert_eq!(parse_report(&p), Some(Event::Battery { percent: 35, charging: true }));
    }

    #[test]
    fn volume_event_and_reply() {
        let ev = pad(&[0x02, 0x0C, 0x06, 0x00, 0x08, 0x00, 0x1F, 0x00, 0x15]);
        assert_eq!(parse_report(&ev), Some(Event::Volume(31)));
        let reply = pad(&[0x02, 0x0C, 0x06, 0x00, 0x08, 0x0D, 0x13, 0x00, 0x0D]);
        assert_eq!(parse_report(&reply), Some(Event::Volume(19)));
    }

    #[test]
    fn balance_event() {
        let p = pad(&[0x02, 0x0C, 0x04, 0x00, 0x0A, 0x00, 0x06]);
        assert_eq!(parse_report(&p), Some(Event::Balance(6)));
    }

    #[test]
    fn active_states() {
        let off = pad(&[0x02, 0x0C, 0x04, 0x00, 0x12, 0x00, 0x05]);
        let on = pad(&[0x02, 0x0C, 0x04, 0x00, 0x12, 0x00, 0x00]);
        assert_eq!(parse_report(&off), Some(Event::Active(false)));
        assert_eq!(parse_report(&on), Some(Event::Active(true)));
    }

    #[test]
    fn firmware_and_name_match_live_replies() {
        // Captured from the headset: firmware 1.3.0.24, name "A50".
        let fw = pad(&[
            0x02, 0x0C, 0x0D, 0x00, 0x04, 0x1D, 0x01, 0x03, 0x00, 0x18, 0x04, b'm', b'a', b'i', b'n',
        ]);
        let name = pad(&[0x02, 0x0C, 0x07, 0x00, 0x05, 0x2D, 0x03, b'A', b'5', b'0']);
        assert_eq!(parse_report(&fw), Some(Event::Firmware("1.3.0.24".into())));
        assert_eq!(parse_report(&name), Some(Event::Name("A50".into())));
    }

    #[test]
    fn firmware_error_and_garbage_are_ignored() {
        let err = pad(b"\x02\x01\x1f\x01\x00\x00\x00HID_ERROR_UNHANDLED_PACKET");
        assert_eq!(parse_report(&err), None);
        assert_eq!(parse_report(&[0x02, 0x0C]), None);
        // dock-state event (idx 0c) is not modelled
        let dock = pad(&[0x02, 0x0C, 0x07, 0x00, 0x0C, 0x00, 0x00, 0x00, 0x00, 0x0A]);
        assert_eq!(parse_report(&dock), None);
    }
}
