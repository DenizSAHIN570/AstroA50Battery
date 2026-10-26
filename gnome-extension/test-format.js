// gnome-extension/test-format.js — run with: gjs -m gnome-extension/test-format.js
import {
    formatStatus, balanceLabel, balanceFraction, batteryIconName, batteryColorClass,
    accentClass, chipText, litSegments, formatEstimate,
} from './format.js';

const TESTS = [
    [formatStatus('charging'),            'Charging'],
    [formatStatus('fully-charged'),       'Fully charged'],
    [formatStatus('discharging'),         'Discharging'],
    [formatStatus('weird'),               'weird'],
    [balanceLabel(6),                     'Centred'],
    [balanceLabel(0),                     'Voice +6'],
    [balanceLabel(3),                     'Voice +3'],
    [balanceLabel(12),                    'Game +6'],
    [balanceLabel(10),                    'Game +4'],
    [balanceFraction(0),                  0],
    [balanceFraction(6),                  0.5],
    [balanceFraction(12),                 1],
    [balanceFraction(99),                 1],
    [batteryIconName(35, 'discharging'),  'battery-good-symbolic'],
    [batteryIconName(35, 'charging'),     'battery-good-charging-symbolic'],
    [batteryIconName(80, 'discharging'),  'battery-full-symbolic'],
    [batteryIconName(5, 'discharging'),   'battery-caution-symbolic'],
    [batteryIconName(5, 'charging'),      'battery-caution-charging-symbolic'],
    [batteryIconName(100, 'fully-charged'), 'battery-full-charged-symbolic'],
    [batteryColorClass(35, 'discharging'), 'battery-orange'],
    [batteryColorClass(80, 'discharging'), 'battery-green'],
    [batteryColorClass(10, 'discharging'), 'battery-red'],
    [batteryColorClass(10, 'fully-charged'), 'battery-green'],
    [accentClass(30, 'discharging'),      'astro-accent-orange'],
    [accentClass(30, 'charging'),         'astro-accent-green'],
    [accentClass(80, 'discharging'),      'astro-accent-green'],
    [accentClass(10, 'discharging'),      'astro-accent-red'],
    [chipText('discharging', true),       'Discharging'],
    [chipText('charging', true),          'Charging'],
    [chipText('discharging', false),      'Off / docked'],
    [litSegments(30),                     6],
    [litSegments(0),                      0],
    [litSegments(100),                    20],
    [litSegments(35),                     7],
    [litSegments(31),                     7],
    [formatEstimate('discharging', 26400, 0), '≈ 7h 20m left'],
    [formatEstimate('discharging', 2700, 0),  '≈ 45m left'],
    [formatEstimate('discharging', 20, 0),    '≈ 1m left'],
    [formatEstimate('discharging', 0, 0),     'Calculating…'],
    [formatEstimate('charging', 0, 7800),     '≈ 2h 10m to full'],
    [formatEstimate('charging', 0, 0),        'Calculating…'],
    [formatEstimate('fully-charged', 0, 0),   'Fully charged'],
    [formatEstimate('unknown', 5, 5),         ''],
];

let passed = 0;
for (const [got, expected] of TESTS) {
    if (got === expected) {
        passed++;
    } else {
        print(`FAIL: got "${got}", expected "${expected}"`);
    }
}
print(`${passed}/${TESTS.length} passed`);
if (passed !== TESTS.length) imports.system.exit(1);
