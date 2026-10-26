mod dbus;
mod hid;

use anyhow::{bail, Result};
use dbus::State;
use hid::{Event, Headset};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tracing_subscriber::EnvFilter;
use zbus::Connection;

/// How long to wait for the first battery reply before re-sending the queries.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_QUERY_TRIES: u32 = 3;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("astro_battery=info".parse()?))
        .init();

    tracing::info!("astro-battery daemon starting");

    let state = Arc::new(Mutex::new(State::default()));
    let conn = dbus::build_connection(Arc::clone(&state)).await?;
    tracing::info!("DBus service registered as {}", dbus::SERVICE_NAME);

    tokio::select! {
        _ = run(&conn, &state) => {},
        _ = tokio::signal::ctrl_c() => tracing::info!("Shutting down"),
    }
    Ok(())
}

/// Find the headset, serve it until the node dies, repeat. The base station
/// re-enumerates on USB now and then, which surfaces as EIO on the open fd.
async fn run(conn: &Connection, state: &Arc<Mutex<State>>) {
    let mut announced_absent = false;
    loop {
        let Some(path) = hid::find_node() else {
            if !announced_absent {
                tracing::info!("No A50 control interface found, waiting");
                announced_absent = true;
            }
            sleep(Duration::from_secs(1)).await;
            continue;
        };
        announced_absent = false;
        tracing::info!("Using {}", path.display());

        if let Err(e) = session(conn, state, &path).await {
            tracing::warn!("Session ended: {e:#}");
        }
        set_absent(conn, state).await;
        sleep(Duration::from_millis(300)).await; // let a re-enumerated node settle
    }
}

async fn session(conn: &Connection, state: &Arc<Mutex<State>>, path: &std::path::Path) -> Result<()> {
    let headset = Headset::open(path)?;
    send_queries(&headset).await?;

    let mut present = false;
    let mut tries = 1;
    loop {
        // Until the first battery reply arrives, don't wait forever: resend.
        let report = if present {
            headset.read().await?
        } else {
            match timeout(REPLY_TIMEOUT, headset.read()).await {
                Ok(r) => r?,
                Err(_) => {
                    if tries >= MAX_QUERY_TRIES {
                        bail!("no battery reply after {tries} queries");
                    }
                    tries += 1;
                    send_queries(&headset).await?;
                    continue;
                }
            }
        };

        let Some(event) = hid::parse_report(&report) else {
            tracing::trace!("ignored report {:02x?}", &report[..report.len().min(12)]);
            continue;
        };
        tracing::debug!("{event:?}");

        let mut requery = false;
        let mut newly_present = false;
        {
            let mut s = state.lock().unwrap();
            match event {
                Event::Battery { percent, charging } => {
                    s.percentage = percent;
                    s.status = match (charging, percent) {
                        (true, 100) => "fully-charged",
                        (true, _) => "charging",
                        (false, _) => "discharging",
                    }
                    .to_string();
                    if !s.is_present {
                        s.is_present = true;
                        newly_present = true;
                    }
                    present = true;
                }
                Event::Volume(pos) => s.volume = ((pos as u32 * 100 + 15) / 31) as u8,
                Event::Balance(b) => s.balance = b,
                Event::Active(active) => {
                    requery = active && !s.is_active;
                    s.is_active = active;
                }
            }
        }

        if newly_present {
            tracing::info!("A50 present");
            dbus::emit_device_added(conn).await;
        }
        dbus::publish(conn).await;
        // Powering on re-announces the battery itself; querying again is cheap insurance.
        if requery {
            send_queries(&headset).await?;
        }
    }
}

async fn send_queries(headset: &Headset) -> Result<()> {
    for q in hid::QUERIES {
        headset.write(&q).await?;
    }
    Ok(())
}

async fn set_absent(conn: &Connection, state: &Arc<Mutex<State>>) {
    let was_present = {
        let mut s = state.lock().unwrap();
        let was = s.is_present;
        s.is_present = false;
        s.status = "unknown".to_string();
        was
    };
    if was_present {
        tracing::info!("A50 gone");
        dbus::publish(conn).await;
        dbus::emit_device_removed(conn).await;
    }
}
