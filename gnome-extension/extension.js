import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import St from 'gi://St';

import {
    formatStatus, balanceLabel, balanceFraction, batteryIconName, batteryColorClass,
    BALANCE_MID,
} from './format.js';

const DBUS_NAME  = 'com.mousewatch.AstroBattery';
const DBUS_PATH  = '/com/mousewatch/AstroBattery/device0';
const DBUS_IFACE = 'com.mousewatch.Battery1';

const INACTIVE_OPACITY = 128;

/**
 * "V ──┃── G" balance bar: a track, a centre tick and a marker that slides
 * from the voice end (left) to the game end (right).
 */
function makeBalanceBar(width, height) {
    const markerW = 4;
    const trackH = 2;

    const widget = new St.Widget({
        layout_manager: new Clutter.FixedLayout(),
        width,
        height,
        y_align: Clutter.ActorAlign.CENTER,
    });
    const track = new St.Widget({style_class: 'astro-balance-track', width, height: trackH});
    track.set_position(0, Math.floor((height - trackH) / 2));
    const tick = new St.Widget({style_class: 'astro-balance-tick', width: 1, height});
    tick.set_position(Math.floor(width / 2), 0);
    const marker = new St.Widget({style_class: 'astro-balance-marker', width: markerW, height});
    widget.add_child(track);
    widget.add_child(tick);
    widget.add_child(marker);

    const setBalance = balance =>
        marker.set_position(Math.round(balanceFraction(balance) * (width - markerW)), 0);
    setBalance(BALANCE_MID);

    const box = new St.BoxLayout({y_align: Clutter.ActorAlign.CENTER});
    const letter = text => new St.Label({
        text,
        style_class: 'astro-balance-letter',
        y_align: Clutter.ActorAlign.CENTER,
    });
    box.add_child(letter('V'));
    const gap = () => new St.Widget({width: 4});
    box.add_child(gap());
    box.add_child(widget);
    box.add_child(gap());
    box.add_child(letter('G'));

    return {actor: box, setBalance};
}

export default class AstroA50BatteryExtension extends Extension {
    enable() {
        // Indicator button: [battery icon] [NN%] [V --|-- G]
        this._indicator = new PanelMenu.Button(0.0, 'Astro A50 Battery', false);

        this._box = new St.BoxLayout({style_class: 'panel-status-menu-box'});
        this._icon = new St.Icon({
            icon_name: 'battery-missing-symbolic',
            style_class: 'system-status-icon',
        });
        this._label = new St.Label({
            text: '–',
            y_align: Clutter.ActorAlign.CENTER,
        });
        this._panelBalance = makeBalanceBar(28, 10);
        this._panelBalance.actor.set_style('margin-left: 6px;');
        this._box.add_child(this._icon);
        this._box.add_child(this._label);
        this._box.add_child(this._panelBalance.actor);
        this._indicator.add_child(this._box);

        // Popup rows
        const menu = this._indicator.menu;
        this._nameItem = new PopupMenu.PopupMenuItem('');
        menu.addMenuItem(this._nameItem);
        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._batteryItem = new PopupMenu.PopupMenuItem('');
        this._volumeItem = new PopupMenu.PopupMenuItem('');
        this._balanceItem = new PopupMenu.PopupMenuItem('');
        menu.addMenuItem(this._batteryItem);
        menu.addMenuItem(this._volumeItem);
        menu.addMenuItem(this._balanceItem);

        this._menuBalance = makeBalanceBar(140, 12);
        const barItem = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        barItem.add_child(this._menuBalance.actor);
        menu.addMenuItem(barItem);

        menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());
        this._versionItem = new PopupMenu.PopupMenuItem('');
        this._versionItem.sensitive = false;
        menu.addMenuItem(this._versionItem);

        Main.panel.addToStatusArea(this.uuid, this._indicator);
        this._indicator.hide();

        // DBus proxy — async so we never block the compositor's main loop
        Gio.DBusProxy.new_for_bus(
            Gio.BusType.SESSION,
            Gio.DBusProxyFlags.DO_NOT_AUTO_START,
            null,
            DBUS_NAME,
            DBUS_PATH,
            DBUS_IFACE,
            null,
            (_obj, res) => {
                try {
                    this._proxy = Gio.DBusProxy.new_for_bus_finish(res);
                } catch (e) {
                    console.error('[astro-a50-battery] proxy creation failed:', e);
                    this._proxy = null;
                    return;
                }

                if (!this._indicator)
                    return; // extension was disabled before proxy completed

                this._propsChangedId = this._proxy.connect('g-properties-changed', () => {
                    this._update();
                });
                this._nameOwnerId = this._proxy.connect('notify::g-name-owner', () => {
                    this._update();
                });

                this._update();
            }
        );
    }

    disable() {
        if (this._proxy) {
            if (this._propsChangedId) {
                this._proxy.disconnect(this._propsChangedId);
                this._propsChangedId = null;
            }
            if (this._nameOwnerId) {
                this._proxy.disconnect(this._nameOwnerId);
                this._nameOwnerId = null;
            }
            this._proxy = null;
        }
        this._indicator?.destroy();
        this._indicator = null;
    }

    _prop(name, fallback) {
        return this._proxy.get_cached_property(name)?.unpack() ?? fallback;
    }

    _update() {
        try {
            if (!this._proxy || !this._indicator) return;
            if (!this._proxy.g_name_owner || !this._prop('IsPresent', false)) {
                this._indicator.hide();
                return;
            }

            const pct     = this._prop('Percentage', 0);
            const status  = this._prop('Status', 'unknown');
            const active  = this._prop('IsActive', true);
            const volume  = this._prop('Volume', 0);
            const balance = this._prop('Balance', BALANCE_MID);
            const name    = this._prop('DeviceName', '');

            // Top bar
            this._icon.icon_name = batteryIconName(pct, status);
            ['battery-green', 'battery-orange', 'battery-red'].forEach(c =>
                this._box.remove_style_class_name(c));
            this._box.add_style_class_name(batteryColorClass(pct, status));
            this._label.text = `${pct}%`;
            this._panelBalance.setBalance(balance);
            this._box.opacity = active ? 255 : INACTIVE_OPACITY;

            // Popup
            this._nameItem.label.text = name;
            this._batteryItem.label.text =
                `Battery: ${pct}%  ·  ${formatStatus(status)}` +
                (active ? '' : '  (headset off or docked)');
            this._volumeItem.label.text = `Volume: ${volume}%`;
            this._balanceItem.label.text = `Balance: ${balanceLabel(balance)}`;
            this._menuBalance.setBalance(balance);

            const daemonVer = this._prop('DaemonVersion', '—');
            const extVer = this.metadata['version-name'] ?? this.metadata.version;
            this._versionItem.label.text = `Daemon: v${daemonVer}  ·  Extension: v${extVer}`;

            this._indicator.show();
        } catch (e) {
            console.error('[astro-a50-battery] _update error:', e);
        }
    }
}
