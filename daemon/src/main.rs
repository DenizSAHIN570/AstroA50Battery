mod dbus;
mod hid;
mod predictor;

use anyhow::{bail, Result};
use dbus::State;
use hid::{Event, Headset};
use predictor::StepPredictor;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::time::{interval, sleep, timeout, MissedTickBehavior};
use tracing_subscriber::EnvFilter;
use zbus::Connection;

/// How long to wait for the first battery reply before re-sending the queries.
const REPLY_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_QUERY_TRIES: u32 = 3;
/// The estimates shrink between battery events; refresh them this often.
const ESTIMATE_REFRESH: Duration = Duration::from_secs(30);

type SharedState = Arc<Mutex<State>>;
type SharedPredictor = Arc<Mutex<StepPredictor>>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("astro_battery=info".parse()?))
        .init();

    tracing::info!("astro-battery daemon starting");

    let state: SharedState = Arc::new(Mutex::new(State::default()));
    let predictor: SharedPredictor = Arc::new(Mutex::new(StepPredictor::new(
        predictor::default_store(),
        Instant::now(),
    )));
    let conn = dbus::build_connection(Arc::clone(&state)).await?;
    tracing::info!("DBus service registered as {}", dbus::SERVICE_NAME);

    tokio::select! {
        _ = run(&conn, &state, &predictor) => {},
        _ = refresh_estimates(&conn, &state, &predictor) => {},
        _ = tokio::signal::ctrl_c() => tracing::info!("Shutting down"),
    }
    Ok(())
}

/// Find the headset, serve it until the node dies, repeat. The base station
/// re-enumerates on USB now and then, which surfaces as EIO on the open fd.
async fn run(conn: &Connection, state: &SharedState, predictor: &SharedPredictor) {
    let mut announced_absent = false;
    let mut last_failure: Option<String> = None;
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
        if last_failure.is_none() {
            tracing::info!("Using {}", path.display());
        }

        if let Err(e) = session(conn, state, predictor, &path).await {
            // Log each distinct failure once; a permanent one (e.g. no permission) would
            // otherwise repeat on every retry.
            let msg = format!("{e:#}");
            if last_failure.as_deref() != Some(msg.as_str()) {
                tracing::warn!("Session ended: {msg}");
            }
            last_failure = Some(msg);
        }
        let had_session = state.lock().unwrap().is_present;
        set_absent(conn, state, predictor).await;
        if had_session {
            last_failure = None;
            // The node usually comes straight back after a re-enumeration.
            sleep(Duration::from_millis(300)).await;
        } else {
            // Never worked since the last success: don't hammer a node that keeps failing.
            sleep(Duration::from_secs(5)).await;
        }
    }
}

async fn session(
    conn: &Connection,
    state: &SharedState,
    predictor: &SharedPredictor,
    path: &std::path::Path,
) -> Result<()> {
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

        // Feed the predictor first (its lock is never held together with the state lock).
        let (time_to_empty, time_to_full) = {
            let now = Instant::now();
            let mut p = predictor.lock().unwrap();
            match &event {
                Event::Battery { percent, charging } => p.push(*percent, *charging, now),
                Event::Active(active) => p.set_active(*active, now),
                _ => {}
            }
            p.estimates(now)
        };

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
                Event::Firmware(v) => s.firmware_version = v,
                Event::Name(n) => s.device_name = format!("Astro {n}"),
                Event::Active(active) => {
                    requery = active && !s.is_active;
                    s.is_active = active;
                }
            }
            s.time_to_empty = time_to_empty;
            s.time_to_full = time_to_full;
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

async fn set_absent(conn: &Connection, state: &SharedState, predictor: &SharedPredictor) {
    {
        // Time passes unobserved while the node is gone; don't count it as use.
        let mut p = predictor.lock().unwrap();
        p.set_active(false, Instant::now());
        p.forget_position();
    }
    let was_present = {
        let mut s = state.lock().unwrap();
        let was = s.is_present;
        s.is_present = false;
        s.status = "unknown".to_string();
        s.time_to_empty = 0;
        s.time_to_full = 0;
        was
    };
    if was_present {
        tracing::info!("A50 gone");
        dbus::publish(conn).await;
        dbus::emit_device_removed(conn).await;
    }
}

/// Keep TimeToEmpty/TimeToFull moving between battery events (a 5% step takes about an hour).
async fn refresh_estimates(conn: &Connection, state: &SharedState, predictor: &SharedPredictor) {
    let mut ticker = interval(ESTIMATE_REFRESH);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        ticker.tick().await;
        if !state.lock().unwrap().is_present {
            continue;
        }
        let (time_to_empty, time_to_full) = predictor.lock().unwrap().estimates(Instant::now());
        let changed = {
            let mut s = state.lock().unwrap();
            let changed = s.time_to_empty != time_to_empty || s.time_to_full != time_to_full;
            s.time_to_empty = time_to_empty;
            s.time_to_full = time_to_full;
            changed
        };
        if changed {
            dbus::publish_estimates(conn).await;
        }
    }
}
