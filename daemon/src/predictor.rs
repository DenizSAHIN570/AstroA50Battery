//! Time-to-empty / time-to-full from the A50's coarse battery reports.
//!
//! The headset reports only a percentage in steps of 5, and only when it changes (no voltage), so
//! this times how long each 5% step takes instead of fitting a curve. The shown value is
//! `ceil(raw / 5) * 5`, so right after it changes the true level sits at the edge of the new bucket
//! (the shown value when draining, shown − 5 when charging) and then moves through it at the
//! learned rate.
//!
//! Discharge time counts only while the headset is active ("hours of use left": nothing drains
//! while it is off or docked). Charge time counts wall-clock time, since docking charges it.

use std::path::PathBuf;
use std::time::Instant;

const STEP_PCT: f64 = 5.0;
/// Weight of the newest step in the smoothed rate.
const RATE_ALPHA: f64 = 0.3;
/// Steps implying a rate outside this range are treated as glitches and ignored.
const MIN_RATE_PCT_PER_HR: f64 = 0.2;
const MAX_RATE_PCT_PER_HR: f64 = 100.0;

#[derive(Clone, Copy)]
struct Boundary {
    /// Value of the relevant clock (see `StepPredictor::clock`) when the step was seen.
    clock: f64,
}

pub struct StepPredictor {
    last_tick: Instant,
    active: bool,
    active_secs: f64,
    wall_secs: f64,
    /// Direction currently being tracked; None until the first report.
    charging: Option<bool>,
    last_pct: Option<u8>,
    /// Where the last observed step change happened, if we saw one in this direction.
    boundary: Option<Boundary>,
    /// Smoothed rates in %/s.
    discharge_rate: Option<f64>,
    charge_rate: Option<f64>,
    store: Option<PathBuf>,
}

/// `$XDG_DATA_HOME/astro-battery/rates`, falling back to `~/.local/share/...`.
pub fn default_store() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))?;
    Some(base.join("astro-battery").join("rates"))
}

impl StepPredictor {
    pub fn new(store: Option<PathBuf>, now: Instant) -> Self {
        let (discharge_rate, charge_rate) = store.as_deref().map(load_rates).unwrap_or((None, None));
        Self {
            last_tick: now,
            active: true,
            active_secs: 0.0,
            wall_secs: 0.0,
            charging: None,
            last_pct: None,
            boundary: None,
            discharge_rate,
            charge_rate,
            store,
        }
    }

    fn advance(&mut self, now: Instant) {
        let dt = now.saturating_duration_since(self.last_tick).as_secs_f64();
        self.wall_secs += dt;
        if self.active {
            self.active_secs += dt;
        }
        self.last_tick = now;
    }

    fn clock(&self, charging: bool) -> f64 {
        if charging { self.wall_secs } else { self.active_secs }
    }

    pub fn set_active(&mut self, active: bool, now: Instant) {
        self.advance(now);
        self.active = active;
    }

    /// Forget where we are inside the current step (device vanished; time passed unobserved).
    pub fn forget_position(&mut self) {
        self.boundary = None;
        self.last_pct = None;
        self.charging = None;
    }

    pub fn push(&mut self, pct: u8, charging: bool, now: Instant) {
        self.advance(now);

        if self.charging != Some(charging) {
            self.boundary = None;
            self.charging = Some(charging);
        }

        if let Some(last) = self.last_pct.filter(|&l| l != pct) {
            let expected: i16 = if charging { 5 } else { -5 };
            if pct as i16 - last as i16 == expected {
                let clock = self.clock(charging);
                if let Some(b) = self.boundary {
                    self.add_sample(charging, clock - b.clock);
                }
                self.boundary = Some(Boundary { clock });
            } else {
                // Skipped or reversed steps: we no longer know when the bucket changed.
                self.boundary = None;
            }
        }
        self.last_pct = Some(pct);
    }

    fn add_sample(&mut self, charging: bool, elapsed_secs: f64) {
        if elapsed_secs < 1.0 {
            return;
        }
        let rate = STEP_PCT / elapsed_secs;
        if !(MIN_RATE_PCT_PER_HR..=MAX_RATE_PCT_PER_HR).contains(&(rate * 3600.0)) {
            return;
        }
        let slot = if charging { &mut self.charge_rate } else { &mut self.discharge_rate };
        *slot = Some(match *slot {
            Some(prev) => RATE_ALPHA * rate + (1.0 - RATE_ALPHA) * prev,
            None => rate,
        });
        if let Some(path) = &self.store {
            save_rates(path, self.discharge_rate, self.charge_rate);
        }
    }

    /// Best guess of the true level inside the current 5% bucket.
    fn level(&self, pct: u8, charging: bool, rate: f64) -> f64 {
        let hi = pct as f64;
        let lo = (hi - STEP_PCT).max(0.0);
        match self.boundary {
            Some(b) => {
                let elapsed = self.clock(charging) - b.clock;
                if charging {
                    (lo + rate * elapsed).clamp(lo, hi)
                } else {
                    (hi - rate * elapsed).clamp(lo, hi)
                }
            }
            None => (lo + hi) / 2.0,
        }
    }

    /// `(seconds to empty, seconds to full)`; 0 where unknown or not applicable.
    pub fn estimates(&mut self, now: Instant) -> (u32, u32) {
        self.advance(now);
        let (Some(pct), Some(charging)) = (self.last_pct, self.charging) else {
            return (0, 0);
        };
        if charging {
            let Some(rate) = self.charge_rate.filter(|_| pct < 100) else { return (0, 0) };
            let level = self.level(pct, true, rate);
            (0, ((100.0 - level) / rate) as u32)
        } else {
            let Some(rate) = self.discharge_rate.filter(|_| pct > 0) else { return (0, 0) };
            let level = self.level(pct, false, rate);
            (((level / rate) as u32), 0)
        }
    }
}

fn load_rates(path: &std::path::Path) -> (Option<f64>, Option<f64>) {
    let Ok(text) = std::fs::read_to_string(path) else { return (None, None) };
    let (mut discharge, mut charge) = (None, None);
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else { continue };
        let Ok(per_hr) = value.trim().parse::<f64>() else { continue };
        if !(MIN_RATE_PCT_PER_HR..=MAX_RATE_PCT_PER_HR).contains(&per_hr) {
            continue;
        }
        match key.trim() {
            "discharge_pct_per_hr" => discharge = Some(per_hr / 3600.0),
            "charge_pct_per_hr" => charge = Some(per_hr / 3600.0),
            _ => {}
        }
    }
    (discharge, charge)
}

fn save_rates(path: &std::path::Path, discharge: Option<f64>, charge: Option<f64>) {
    let mut text = String::new();
    if let Some(r) = discharge {
        text.push_str(&format!("discharge_pct_per_hr={:.4}\n", r * 3600.0));
    }
    if let Some(r) = charge {
        text.push_str(&format!("charge_pct_per_hr={:.4}\n", r * 3600.0));
    }
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(path, text) {
        tracing::warn!("Could not save battery rates to {}: {e}", path.display());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    fn near(got: u32, want: u32) -> bool {
        (got as i64 - want as i64).abs() <= 2
    }

    #[test]
    fn nothing_known_gives_zeros() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        assert_eq!(p.estimates(t0), (0, 0));
        p.push(35, false, t0);
        assert_eq!(p.estimates(at(t0, 100)), (0, 0));
    }

    #[test]
    fn discharge_rate_comes_from_two_complete_steps() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0)); // mid-step when the daemon started
        p.push(30, false, at(t0, 3600)); // first boundary: no rate yet
        assert_eq!(p.estimates(at(t0, 3600)), (0, 0));
        p.push(25, false, at(t0, 7200)); // one full step = 5% per hour
        assert!(near(p.estimates(at(t0, 7200)).0, 5 * 3600)); // 25% at 5%/h
        // 30 min into the step the level has fallen 2.5%
        assert!(near(p.estimates(at(t0, 7200 + 1800)).0, 4 * 3600 + 1800));
    }

    #[test]
    fn level_never_drops_below_the_bucket_floor() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0));
        p.push(30, false, at(t0, 3600));
        p.push(25, false, at(t0, 7200));
        // Three hours into a one-hour step: level clamps at 20%, not negative or shrinking further.
        assert!(near(p.estimates(at(t0, 7200 + 3 * 3600)).0, 4 * 3600));
    }

    #[test]
    fn headset_off_time_is_not_counted_while_discharging() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0));
        p.push(30, false, at(t0, 3600));
        p.set_active(false, at(t0, 3600));
        p.set_active(true, at(t0, 3600 + 7200)); // off for two hours
        p.push(25, false, at(t0, 3600 + 7200 + 1800)); // only 30 min of use in this step
        // 5% per 30 min of use = 10%/h -> 25% left = 2.5 h
        assert!(near(p.estimates(at(t0, 3600 + 7200 + 1800)).0, 9000));
    }

    #[test]
    fn charge_time_counts_wall_clock_even_when_docked() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.set_active(false, t0); // docked
        p.push(30, true, at(t0, 0));
        p.push(35, true, at(t0, 600));
        p.push(40, true, at(t0, 1200)); // 5% per 10 min
        // level just crossed to 35%; 65% to go at 5%/600 s
        assert!(near(p.estimates(at(t0, 1200)).1, 7800));
        assert_eq!(p.estimates(at(t0, 1200)).0, 0);
    }

    #[test]
    fn changing_direction_drops_the_current_step() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0));
        p.push(30, false, at(t0, 3600));
        p.push(25, false, at(t0, 7200));
        p.push(25, true, at(t0, 7300)); // cable in: no charge rate learned yet
        assert_eq!(p.estimates(at(t0, 7300)), (0, 0));
        p.push(25, false, at(t0, 8000)); // cable out
        assert!(p.estimates(at(t0, 8000)).0 > 0); // discharge rate survived
    }

    #[test]
    fn skipped_steps_reset_the_boundary() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0));
        p.push(30, false, at(t0, 3600));
        p.push(20, false, at(t0, 7200)); // two steps at once: events were missed
        assert_eq!(p.estimates(at(t0, 7200)), (0, 0));
    }

    #[test]
    fn implausibly_fast_steps_are_ignored() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.push(35, false, at(t0, 0));
        p.push(30, false, at(t0, 10));
        p.push(25, false, at(t0, 12)); // 5% in 2 s
        assert_eq!(p.estimates(at(t0, 12)), (0, 0));
    }

    #[test]
    fn rates_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("astro-battery-test-{}", std::process::id()));
        let store = dir.join("rates");
        let t0 = Instant::now();
        {
            let mut p = StepPredictor::new(Some(store.clone()), t0);
            p.push(35, false, at(t0, 0));
            p.push(30, false, at(t0, 3600));
            p.push(25, false, at(t0, 7200)); // learns 5%/h and saves it
        }
        let mut fresh = StepPredictor::new(Some(store.clone()), t0);
        fresh.push(60, false, t0);
        // no steps seen yet, but the stored rate gives an estimate: bucket middle 57.5% at 5%/h
        assert!(near(fresh.estimates(t0).0, (57.5 / 5.0 * 3600.0) as u32));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn full_and_empty_edges_give_zero() {
        let t0 = Instant::now();
        let mut p = StepPredictor::new(None, t0);
        p.discharge_rate = Some(5.0 / 3600.0);
        p.charge_rate = Some(5.0 / 3600.0);
        p.push(100, true, t0);
        assert_eq!(p.estimates(t0), (0, 0));
        p.push(0, false, at(t0, 1));
        assert_eq!(p.estimates(at(t0, 1)), (0, 0));
    }
}
