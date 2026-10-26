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

export function batteryColorClass(pct, status) {
    if (status === 'fully-charged' || pct > 50) return 'battery-green';
    return pct > 10 ? 'battery-orange' : 'battery-red';
}
