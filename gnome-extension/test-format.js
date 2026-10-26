// gnome-extension/test-format.js — run with: gjs -m gnome-extension/test-format.js
import {
    formatStatus, balanceLabel, balanceFraction, batteryIconName, batteryColorClass,
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
