import {Extension} from 'resource:///org/gnome/shell/extensions/extension.js';
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
import * as PanelMenu from 'resource:///org/gnome/shell/ui/panelMenu.js';
import * as PopupMenu from 'resource:///org/gnome/shell/ui/popupMenu.js';
import Clutter from 'gi://Clutter';
import Gio from 'gi://Gio';
import Pango from 'gi://Pango';
import St from 'gi://St';

import {
    formatStatus, balanceLabel, balanceFraction, batteryIconName, batteryColorClass,
    accentClass, chipText, litSegments, BALANCE_MID,
} from './format.js';

const DBUS_NAME  = 'com.mousewatch.AstroBattery';
const DBUS_PATH  = '/com/mousewatch/AstroBattery/device0';
const DBUS_IFACE = 'com.mousewatch.Battery1';

const INACTIVE_OPACITY = 128;
const POPUP_WIDTH = 380;
const SEGMENTS = 20;
const ACCENTS = ['astro-accent-green', 'astro-accent-orange', 'astro-accent-red'];

function setAccent(actor, accent) {
    ACCENTS.forEach(c => actor.remove_style_class_name(c));
    actor.add_style_class_name(accent);
}

function makeLabel(text, styleClass) {
    const label = new St.Label({text, style_class: styleClass, y_align: Clutter.ActorAlign.CENTER});
    // Never cut text short with an ellipsis; the rows have room.
    label.clutter_text.set_ellipsize(Pango.EllipsizeMode.NONE);
    return label;
}

function makeSpacer() {
    return new St.Widget({x_expand: true});
}

function makeRow(...children) {
    const row = new St.BoxLayout({x_expand: true});
    children.forEach(c => row.add_child(c));
    return row;
}

/**
 * Thin horizontal progress bar that fills the width it is given.
 * set(fraction) takes 0..1.
 */
function makeBar(height) {
    const track = new St.Widget({
        layout_manager: new Clutter.FixedLayout(),
        style_class: 'astro-bar',
        x_expand: true,
        height,
    });
    const fill = new St.Widget({style_class: 'astro-bar-fill', height});
    track.add_child(fill);

    let fraction = 0;
    const apply = () => fill.set_width(Math.round(track.width * fraction));
    track.connect('notify::allocation', apply);
    return {
        actor: track,
        set(f) {
            fraction = Math.min(Math.max(f, 0), 1);
            apply();
        },
    };
}

/**
 * Balance track: line, centre tick, sliding marker. Voice end is left, game end right.
 * Pass `width` for a fixed size (top bar); omit it to fill the available width (popup).
 */
function makeBalanceTrack({width = null, height, markerW}) {
    const props = {layout_manager: new Clutter.FixedLayout(), height};
    if (width !== null)
        props.width = width;
    else
        props.x_expand = true;
    const widget = new St.Widget(props);

    const trackH = 2;
    const track = new St.Widget({style_class: 'astro-balance-track', height: trackH});
    const tick = new St.Widget({style_class: 'astro-balance-tick', width: 1, height});
    const marker = new St.Widget({style_class: 'astro-balance-marker', width: markerW, height});
    widget.add_child(track);
    widget.add_child(tick);
    widget.add_child(marker);

    let balance = BALANCE_MID;
    const layout = () => {
        const w = widget.width;
        track.set_width(w);
        track.set_position(0, Math.floor((height - trackH) / 2));
        tick.set_position(Math.floor(w / 2), 0);
        marker.set_position(Math.round(balanceFraction(balance) * (w - markerW)), 0);
    };
    widget.connect('notify::allocation', layout);
    return {
        actor: widget,
        setBalance(b) {
            balance = b;
            layout();
        },
    };
}

/** Top-bar "V --|-- G" balance bar. */
function makeBalanceBar(width, height) {
    const track = makeBalanceTrack({width, height, markerW: 3});
    const box = new St.BoxLayout({y_align: Clutter.ActorAlign.CENTER});
    box.add_child(makeLabel('V', 'astro-balance-letter'));
    box.add_child(new St.Widget({width: 4}));
    box.add_child(track.actor);
    box.add_child(new St.Widget({width: 4}));
    box.add_child(makeLabel('G', 'astro-balance-letter'));
    return {actor: box, setBalance: track.setBalance};
}

export default class AstroA50BatteryExtension extends Extension {
    enable() {
        // Indicator button: [battery icon] [NN%] [V --|-- G]
        // 0.5 centres the popup under the icon (0.0 would hang it off to the right).
        this._indicator = new PanelMenu.Button(0.5, 'Astro A50 Battery', false);

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

        this._buildPopup(this._indicator.menu);

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
        this._w = null;
    }

    /** Header + three cards (battery, volume, balance) + footer, as one non-interactive item. */
    _buildPopup(menu) {
        // GNOME draws the popup in .popup-menu-content (20px radius); this class squares it
        // off and drops its padding so our own card layout supplies the spacing.
        menu.box.add_style_class_name('astro-menu-box');

        const w = this._w = {};
        const pop = new St.BoxLayout({vertical: true, style_class: 'astro-pop', width: POPUP_WIDTH});

        // Header: name + sub-line on the left, status chip on the right
        const titles = new St.BoxLayout({vertical: true});
        w.name = makeLabel('', 'astro-name');
        w.sub = makeLabel('', 'astro-sub');
        titles.add_child(w.name);
        titles.add_child(w.sub);
        w.chip = makeLabel('', 'astro-chip');
        w.chip.y_align = Clutter.ActorAlign.CENTER;
        pop.add_child(makeRow(titles, makeSpacer(), w.chip));

        const cards = new St.BoxLayout({vertical: true, style_class: 'astro-cards'});
        pop.add_child(cards);

        // Battery card: big number left, 20 segments (5% each) right
        const battery = new St.BoxLayout({style_class: 'astro-card'});
        const batteryText = new St.BoxLayout({vertical: true});
        batteryText.add_child(makeLabel('BATTERY', 'astro-lbl'));
        w.big = new St.Label({style_class: 'astro-big', y_align: Clutter.ActorAlign.CENTER});
        batteryText.add_child(w.big);
        w.segments = new St.BoxLayout({style_class: 'astro-segments', y_align: Clutter.ActorAlign.CENTER});
        w.segs = [];
        for (let i = 0; i < SEGMENTS; i++) {
            const seg = new St.Widget({style_class: 'astro-seg'});
            w.segs.push(seg);
            w.segments.add_child(seg);
        }
        battery.add_child(batteryText);
        battery.add_child(makeSpacer());
        battery.add_child(w.segments);
        cards.add_child(battery);
        w.batteryCard = battery;

        // Volume card
        const volume = new St.BoxLayout({vertical: true, style_class: 'astro-card'});
        w.volume = makeLabel('', 'astro-val');
        volume.add_child(makeRow(makeLabel('VOLUME', 'astro-lbl'), makeSpacer(), w.volume));
        w.volumeBar = makeBar(4);
        w.volumeBar.actor.add_style_class_name('astro-bar-gap');
        volume.add_child(w.volumeBar.actor);
        cards.add_child(volume);

        // Balance card
        const balance = new St.BoxLayout({vertical: true, style_class: 'astro-card'});
        w.balance = makeLabel('', 'astro-val');
        balance.add_child(makeRow(makeLabel('BALANCE', 'astro-lbl'), makeSpacer(), w.balance));
        w.balanceTrack = makeBalanceTrack({height: 12, markerW: 4});
        w.balanceTrack.actor.add_style_class_name('astro-bar-gap');
        balance.add_child(w.balanceTrack.actor);
        balance.add_child(makeRow(
            makeLabel('VOICE', 'astro-ends'), makeSpacer(), makeLabel('GAME', 'astro-ends')));
        cards.add_child(balance);

        // Footer
        w.footer = makeLabel('', 'astro-footer');
        pop.add_child(w.footer);

        const item = new PopupMenu.PopupBaseMenuItem({reactive: false, can_focus: false});
        item.add_style_class_name('astro-menu-item');
        item.add_child(pop);
        menu.addMenuItem(item);
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
            const w = this._w;
            const accent = accentClass(pct, status);

            w.name.text = name;
            const firmware = this._prop('FirmwareVersion', '');
            w.sub.text = firmware ? `Firmware ${firmware}` : (active ? 'Connected' : 'Off or docked');
            w.chip.text = chipText(status, active);
            setAccent(w.chip, active ? accent : 'astro-accent-neutral');

            w.big.clutter_text.set_markup(`${pct}<span size="smaller">%</span>`);
            setAccent(w.segments, accent);
            const lit = litSegments(pct);
            w.segs.forEach((seg, i) => {
                if (i < lit)
                    seg.add_style_class_name('astro-seg-on');
                else
                    seg.remove_style_class_name('astro-seg-on');
            });
            w.batteryCard.opacity = active ? 255 : INACTIVE_OPACITY;

            w.volume.text = `${volume}%`;
            w.volumeBar.set(volume / 100);
            w.balance.text = balanceLabel(balance);
            w.balanceTrack.setBalance(balance);

            const daemonVer = this._prop('DaemonVersion', '—');
            const extVer = this.metadata['version-name'] ?? this.metadata.version;
            w.footer.text = `Daemon v${daemonVer}  ·  Extension v${extVer}`;

            this._indicator.show();
        } catch (e) {
            console.error('[astro-a50-battery] _update error:', e);
        }
    }
}
