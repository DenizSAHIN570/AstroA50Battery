use anyhow::Result;
use std::sync::{Arc, Mutex};
use zbus::object_server::SignalEmitter;
use zbus::{connection, interface, Connection};
use zvariant::OwnedObjectPath;

pub const SERVICE_NAME: &str = "com.mousewatch.AstroBattery";
pub const MANAGER_PATH: &str = "/com/mousewatch/AstroBattery";
pub const DEVICE0_PATH: &str = "/com/mousewatch/AstroBattery/device0";

#[derive(Debug, Clone)]
pub struct State {
    /// "Astro" + the product name the headset reports; "Astro A50" until it answers.
    pub device_name: String,
    /// `major.minor.patch.build` from the headset, empty until it answers.
    pub firmware_version: String,
    pub percentage: u8,
    pub status: String,
    /// Seconds; 0 = unknown / not applicable. Empty time counts hours of use only.
    pub time_to_full: u32,
    pub time_to_empty: u32,
    pub is_present: bool,
    /// false while the headset is powered off or docked (battery value is stale-ish).
    pub is_active: bool,
    /// 0..=100, from the 0..=31 wheel position.
    pub volume: u8,
    /// 0 = voice maxed, 6 = middle, 12 = game maxed.
    pub balance: u8,
}

impl Default for State {
    fn default() -> Self {
        Self {
            device_name: "Astro A50".to_string(),
            firmware_version: String::new(),
            percentage: 0,
            status: "unknown".to_string(),
            time_to_full: 0,
            time_to_empty: 0,
            is_present: false,
            is_active: true,
            volume: 0,
            balance: 6,
        }
    }
}

pub struct BatteryDevice {
    pub state: Arc<Mutex<State>>,
}

// Same interface name as the Asus mouse daemon so one extension can consume both;
// the extra IsActive / Volume / Balance properties are headset-only.
#[interface(name = "com.mousewatch.Battery1")]
impl BatteryDevice {
    #[zbus(property)]
    async fn device_name(&self) -> String {
        self.state.lock().unwrap().device_name.clone()
    }

    #[zbus(property)]
    async fn percentage(&self) -> u8 {
        self.state.lock().unwrap().percentage
    }

    #[zbus(property)]
    async fn status(&self) -> String {
        self.state.lock().unwrap().status.clone()
    }

    #[zbus(property)]
    async fn time_to_full(&self) -> u32 {
        self.state.lock().unwrap().time_to_full
    }

    #[zbus(property)]
    async fn time_to_empty(&self) -> u32 {
        self.state.lock().unwrap().time_to_empty
    }

    // The headset reports no voltage; kept at 0 for interface parity with the mouse.
    #[zbus(property)]
    async fn voltage_mv(&self) -> u32 {
        0
    }

    #[zbus(property)]
    async fn is_present(&self) -> bool {
        self.state.lock().unwrap().is_present
    }

    #[zbus(property)]
    async fn is_active(&self) -> bool {
        self.state.lock().unwrap().is_active
    }

    #[zbus(property)]
    async fn volume(&self) -> u8 {
        self.state.lock().unwrap().volume
    }

    #[zbus(property)]
    async fn balance(&self) -> u8 {
        self.state.lock().unwrap().balance
    }

    #[zbus(property)]
    async fn firmware_version(&self) -> String {
        self.state.lock().unwrap().firmware_version.clone()
    }

    #[zbus(property)]
    async fn daemon_version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    #[zbus(signal)]
    pub async fn battery_changed(
        emitter: &SignalEmitter<'_>,
        percentage: u8,
        status: String,
    ) -> zbus::Result<()>;
}

pub struct BatteryManager {
    pub device_paths: Vec<OwnedObjectPath>,
}

#[interface(name = "com.mousewatch.BatteryManager1")]
impl BatteryManager {
    async fn get_devices(&self) -> Vec<OwnedObjectPath> {
        self.device_paths.clone()
    }

    #[zbus(signal)]
    pub async fn device_added(
        emitter: &SignalEmitter<'_>,
        path: OwnedObjectPath,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    pub async fn device_removed(
        emitter: &SignalEmitter<'_>,
        path: OwnedObjectPath,
    ) -> zbus::Result<()>;
}

pub async fn build_connection(state: Arc<Mutex<State>>) -> Result<Connection> {
    let device0 = OwnedObjectPath::try_from(DEVICE0_PATH).expect("valid object path");
    Ok(connection::Builder::session()?
        .name(SERVICE_NAME)?
        .serve_at(DEVICE0_PATH, BatteryDevice { state })?
        .serve_at(MANAGER_PATH, BatteryManager { device_paths: vec![device0] })?
        .build()
        .await?)
}

/// Tell DBus clients that properties changed and emit BatteryChanged.
pub async fn publish(conn: &Connection) {
    let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, BatteryDevice>(DEVICE0_PATH)
        .await
    else {
        return;
    };
    let emitter = iface_ref.signal_emitter();
    let iface = iface_ref.get().await;
    let _ = iface.device_name_changed(emitter).await;
    let _ = iface.firmware_version_changed(emitter).await;
    let _ = iface.percentage_changed(emitter).await;
    let _ = iface.status_changed(emitter).await;
    let _ = iface.time_to_empty_changed(emitter).await;
    let _ = iface.time_to_full_changed(emitter).await;
    let _ = iface.is_present_changed(emitter).await;
    let _ = iface.is_active_changed(emitter).await;
    let _ = iface.volume_changed(emitter).await;
    let _ = iface.balance_changed(emitter).await;
    let (pct, status) = {
        let s = iface.state.lock().unwrap();
        (s.percentage, s.status.clone())
    };
    let _ = BatteryDevice::battery_changed(emitter, pct, status).await;
}

/// Time estimates move on their own between battery events; announce just those.
pub async fn publish_estimates(conn: &Connection) {
    let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, BatteryDevice>(DEVICE0_PATH)
        .await
    else {
        return;
    };
    let emitter = iface_ref.signal_emitter();
    let iface = iface_ref.get().await;
    let _ = iface.time_to_empty_changed(emitter).await;
    let _ = iface.time_to_full_changed(emitter).await;
}

pub async fn emit_device_added(conn: &Connection) {
    emit_manager(conn, true).await;
}

pub async fn emit_device_removed(conn: &Connection) {
    emit_manager(conn, false).await;
}

async fn emit_manager(conn: &Connection, added: bool) {
    let Ok(iface_ref) = conn
        .object_server()
        .interface::<_, BatteryManager>(MANAGER_PATH)
        .await
    else {
        return;
    };
    let path = OwnedObjectPath::try_from(DEVICE0_PATH).expect("valid object path");
    let emitter = iface_ref.signal_emitter();
    let _ = if added {
        BatteryManager::device_added(emitter, path).await
    } else {
        BatteryManager::device_removed(emitter, path).await
    };
}
