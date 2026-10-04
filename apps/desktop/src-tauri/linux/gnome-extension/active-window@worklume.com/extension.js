// Worklume active window (E13-S05): tells the Worklume agent which app is in front on GNOME,
// where Wayland gives apps no way to know. GNOME Shell 45 and later.
//
// - One D-Bus method, Get(with_title) -> (app_id, wm_class, pid, title), on the session bus as
//   com.worklume.ActiveWindow. The title is read only when asked for. Empty values and 0 mean
//   nothing is in front.
// - No signals: a signal reaches every client on the bus, so the agent asks once a second.
// - Only the installed agent may ask: the executable behind the caller's process must be on
//   ALLOWED. Other apps of the same user, and sandboxed ones, get "access denied". Code running
//   as the same user can do worse, so this is defence in depth, not a security boundary.
// - The extension keeps nothing and sends nothing anywhere.
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');

const BUS_NAME = 'com.worklume.ActiveWindow';
const OBJECT_PATH = '/com/worklume/ActiveWindow';
/** Where the Worklume packages install the agent. */
const ALLOWED = ['/usr/bin/worklume-desktop'];
/** What /proc says about an executable replaced, by an update, while it runs. */
const DELETED = ' (deleted)';

const IFACE = `
<node>
  <interface name="com.worklume.ActiveWindow1">
    <method name="Get">
      <arg type="b" direction="in" name="with_title"/>
      <arg type="s" direction="out" name="app_id"/>
      <arg type="s" direction="out" name="wm_class"/>
      <arg type="u" direction="out" name="pid"/>
      <arg type="s" direction="out" name="title"/>
    </method>
  </interface>
</node>`;

export default class ActiveWindowExtension extends Extension {
  enable() {
    this._impl = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
    this._impl.export(Gio.DBus.session, OBJECT_PATH);
    this._name = Gio.bus_own_name_on_connection(
      Gio.DBus.session,
      BUS_NAME,
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
    );
  }

  disable() {
    Gio.bus_unown_name(this._name);
    this._name = 0;
    this._impl.unexport();
    this._impl = null;
  }

  async GetAsync([withTitle], invocation) {
    if (!(await isAgent(invocation.get_sender()))) {
      invocation.return_dbus_error(
        'org.freedesktop.DBus.Error.AccessDenied',
        'Only the Worklume agent may ask',
      );
      return;
    }
    const window = global.display.focus_window;
    if (!window) {
      invocation.return_value(new GLib.Variant('(ssus)', ['', '', 0, '']));
      return;
    }
    const app = Shell.WindowTracker.get_default().get_window_app(window);
    invocation.return_value(
      new GLib.Variant('(ssus)', [
        app && !app.is_window_backed() ? (app.get_id() ?? '') : '',
        window.get_wm_class() ?? '',
        Math.max(window.get_pid(), 0),
        withTitle ? (window.get_title() ?? '') : '',
      ]),
    );
  }
}

/** Whether the D-Bus client `sender` is the installed agent. */
async function isAgent(sender) {
  try {
    const reply = await Gio.DBus.session.call(
      'org.freedesktop.DBus',
      '/org/freedesktop/DBus',
      'org.freedesktop.DBus',
      'GetConnectionUnixProcessID',
      new GLib.Variant('(s)', [sender]),
      null,
      Gio.DBusCallFlags.NONE,
      -1,
      null,
    );
    const [pid] = reply.deepUnpack();
    let exe = GLib.file_read_link(`/proc/${pid}/exe`);
    if (exe.endsWith(DELETED)) exe = exe.slice(0, -DELETED.length);
    return ALLOWED.includes(exe);
  } catch {
    return false;
  }
}
