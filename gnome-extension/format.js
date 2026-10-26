// Pure helpers, no Shell imports, so they can be tested with `gjs -m test-format.js`.

export const BALANCE_MAX = 12;
export const BALANCE_MID = 6;

export function formatStatus(status) {
    const labels = {
        'charging': 'Charging',
        'discharging': 'Discharging',
        'fully-charged': 'Fully charged',
        'unknown': 'Unknown',
    };
    return labels[status] ?? status;
}

/** 0 = voice maxed, 6 = centred, 12 = game maxed. */
export function balanceLabel(balance) {
    const d = balance - BALANCE_MID;
    if (d === 0) return 'Centred';
    return d < 0 ? `Voice +${-d}` : `Game +${d}`;
}

/** Marker position along the bar, 0 (voice end) .. 1 (game end). */
export function balanceFraction(balance) {
    return Math.min(Math.max(balance, 0), BALANCE_MAX) / BALANCE_MAX;
}

export function batteryIconName(pct, status) {
    if (status === 'fully-charged')
        return 'battery-full-charged-symbolic';
    const level = pct > 50 ? 'full' : pct > 10 ? 'good' : 'caution';
    return status === 'charging'
        ? `battery-${level}-charging-symbolic`
        : `battery-${level}-symbolic`;
}

/** Accent for the popup chip and battery segments: green while charging or full. */
export function accentClass(pct, status) {
    if (status === 'charging' || status === 'fully-charged') return 'astro-accent-green';
    if (pct > 50) return 'astro-accent-green';
    return pct > 10 ? 'astro-accent-orange' : 'astro-accent-red';
}

/** Text for the status chip in the popup header. */
export function chipText(status, active) {
    return active ? formatStatus(status) : 'Off / docked';
}

function formatDuration(seconds) {
    const h = Math.floor(seconds / 3600);
    const m = Math.floor((seconds % 3600) / 60);
    return h > 0 ? `${h}h ${m}m` : `${Math.max(m, 1)}m`;
}

/**
 * Line under the battery number. Empty time counts hours of use (the daemon pauses it while the
 * headset is off or docked); 0 means the daemon has no estimate yet.
 */
export function formatEstimate(status, timeToEmpty, timeToFull) {
    if (status === 'fully-charged') return 'Fully charged';
    if (status === 'charging')
        return timeToFull > 0 ? `≈ ${formatDuration(timeToFull)} to full` : 'Calculating…';
    if (status === 'discharging')
        return timeToEmpty > 0 ? `≈ ${formatDuration(timeToEmpty)} left` : 'Calculating…';
    return '';
}

/** Lit segments out of 20 (one per 5%). */
export function litSegments(pct) {
    return Math.min(Math.max(Math.ceil(pct / 5), 0), 20);
}

export function batteryColorClass(pct, status) {
    if (status === 'fully-charged' || pct > 50) return 'battery-green';
    return pct > 10 ? 'battery-orange' : 'battery-red';
}
