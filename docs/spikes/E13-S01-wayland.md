# E13-S01 spike: Wayland

**Question.** On Wayland, what can the Linux agent measure (idle time and activity, the app in
front, the session lock) and capture (screenshots), on GNOME, KDE and wlroots compositors? What
consent does each need, and what does it cost?

**Answer.** GNOME, the V1 target, is a go with reduced capability:

- **Go:**
  - idle time and per-second activity, from Mutter's `IdleMonitor`;
  - the session lock, from logind, with no change to the E12-S03 adapter;
  - the app in front, through a small Shell extension (prototype below).
- **Go for screenshots, with a change to E13-S03.** Use the **ScreenCast** portal with a
  restore token, not the Screenshot portal. One consent dialog, then capture with no prompt,
  including after logging in again. On every capture, GNOME's Screenshot portal flashes the
  screen, plays the shutter sound, blocks input for half a second and saves a PNG into the
  member's Pictures folder.
- **Not available on any compositor:**
  - a keyboard and mouse split: activity is one combined figure;
  - browser addresses, except through the browser extension (E35).

KDE Plasma 5.27 (Future):

- Idle time, the app in front and the lock are confirmed.
- Two caveats:
  - activity reads as active while a video plays;
  - screen capture could not be confirmed in this workspace.

wlroots compositors (sway) give everything without consent, but no story plans for them.

## How it was tested

- **Environment.** Ubuntu 24.04 in the cloud workspace, with no GPU (Mesa llvmpipe):
  - **GNOME:** GNOME Shell 46.0 and Mutter 46.2. Nested in Xvfb, so dialogs could be seen and
    clicked, and headless. xdg-desktop-portal 1.18.4 with xdg-desktop-portal-gnome 46.2.
  - **KDE:** KWin 5.27.11, nested in Xvfb and on its virtual backend. xdg-desktop-portal-kde
    5.27.11.
  - **wlroots:** sway 1.9 (wlroots 0.17.1), headless with the software renderer.
    xdg-desktop-portal-wlr 0.7.1.
  - **Media:** PipeWire 1.0.5 and WirePlumber 0.4.17.
- **Stand-ins.** Each compositor had its own session bus. A system bus carried a stand-in logind,
  which records the `SetLockedHint` calls it receives, and a stand-in GDM. GNOME Shell creates
  its lock screen only when GDM is present.
- **Throwaway probes.** None of them is kept; the extension and the script are reproduced at the
  end.
  - A Rust Wayland client on `wayland-client` 0.31. It is pure Rust and links no `libwayland`.
  - Python scripts for the portals, Mutter's `IdleMonitor` and the other D-Bus calls.
  - GStreamer's `pipewiresrc` to pull frames.
  - A GNOME Shell extension and a KWin script.
- **Input.** `xdotool` into the nested X window for GNOME and KWin, or wlroots' virtual pointer
  for sway.
- **Not tested:**
  - real hardware, GPUs, HiDPI and several monitors;
  - Plasma 6 (not in Ubuntu 24.04), GNOME 47 or later, and Fedora;
  - systemd user sessions. The portal identifies an unsandboxed app only through its systemd
    scope, so its per-app behaviour comes from the source (see "App identity");
  - capture on KDE: KWin never completed a capture here (see "KDE").

## Capability matrix

"Confirmed" means it was observed in this workspace. "Source" means it was read in the shipped
version's code but not observed.

| Capability                      | GNOME 46 (V1)                                           | KDE Plasma 5.27 (Future)                                                       | sway 1.9, wlroots (not planned)                           |
| ------------------------------- | ------------------------------------------------------- | ------------------------------------------------------------------------------ | --------------------------------------------------------- |
| Session lock                    | logind `LockedHint`, `org.gnome.ScreenSaver`. Confirmed | logind `LockedHint`, `org.freedesktop.ScreenSaver`. Confirmed                  | Not detectable                                            |
| Idle time                       | `IdleMonitor.GetIdletime`, 0.4 ms. Confirmed            | Derived from `ext-idle-notify-v1`. Confirmed                                   | Derived from `ext-idle-notify-v1`. Confirmed              |
| Per-second activity             | The time of every input, with no kind. Confirmed        | Start and end of activity. Confirmed                                           | Start and end of activity. Confirmed                      |
| Keyboard and mouse split        | No                                                      | No                                                                             | No                                                        |
| Idle while a window inhibits it | Counted idle. Confirmed                                 | **Counted active.** Confirmed                                                  | **Counted active.** Confirmed                             |
| Screenshots                     | ScreenCast portal: one dialog, then none. Confirmed     | Portal without a dialog (source). Not confirmed                                | `wlr-screencopy`, no consent, 4.7 ms per frame. Confirmed |
| App in front: ID, PID and title | Shell extension, after the next login. Confirmed        | Plasma window protocol, granted by a desktop file. Or a KWin script. Confirmed | `wlr-foreign-toplevel`, with no PID. Confirmed            |
| Browser address                 | Browser extension (E35)                                 | Browser extension (E35)                                                        | Browser extension (E35)                                   |

## Session and compositor detection (E13-S02)

- `XDG_SESSION_TYPE=wayland` already selects Wayland (`detect.rs`).
- The compositor comes from `XDG_CURRENT_DESKTOP`, split on `:`. Ubuntu sets `ubuntu:GNOME`,
  KDE sets `KDE` and sway sets `sway`. A service on the bus confirms it, because the variable
  can be wrong:
  - `org.gnome.Mutter.IdleMonitor` for GNOME;
  - `org.kde.KWin` for KDE;
  - the `wlr` globals for wlroots.
- **The X11 adapters must stay off on Wayland, even though `DISPLAY` is set for Xwayland.**
  Through Xwayland they would see only X11 apps: their idle time misses input to Wayland
  windows, and `_NET_ACTIVE_WINDOW` is empty whenever a Wayland window is in front.
  `DeviceSensors` already starts them only for X11 sessions.

## Idle time and activity (E13-S04)

### GNOME: Mutter's `IdleMonitor`

`org.gnome.Mutter.IdleMonitor` at `/org/gnome/Mutter/IdleMonitor/Core` answers any
unsandboxed client.

- `GetIdletime` takes 0.37–0.42 ms per call and resets on any input.
- `AddUserActiveWatch` fires once, on the next input. Re-arming it after each firing gives the
  time of every input to within one D-Bus message, but not its kind. A key fired twice, once on
  press and once on release.
- `AddIdleWatch(1000)` fired 1,001 ms after the last input.
- **Wayland idle inhibitors don't affect it.** With a visible window holding
  `zwp_idle_inhibit_manager_v1`, as video players and browsers do, idle time kept growing from
  6.5 s to 8.5 s. Inhibitors taken through gnome-session (the portal's Inhibit) were not tested,
  because there was no gnome-session.

This is the X11 meter's combined case with exact timestamps. The E13-S04 criterion, activity
within 5 points of X11, holds by construction.

### KDE and wlroots: `ext-idle-notify-v1`

- The compositor only reports transitions:
  - `Idled` once the timeout passes with no input;
  - `Resumed` on the next input.

  No value can be read.

- **With a 1 s timeout, `[Resumed, Idled)` is exactly the X11 meter's active time.** The meter's
  active time is the union of `[t, t + 1 s)` over all inputs. On sway, five moves 300 ms
  apart, from about 2.50 s to 3.70 s, gave `Resumed` at 2,503 ms and `Idled` at 4,705 ms:
  2.2 s active. The meter gives the same 2.2 s, from 2.50 s to 4.70 s. KWin behaved the same
  way.
- Idle time is 0 while active, else now − (`Idled` − 1 s).
- **Caveat: a visible window holding an idle inhibitor stops `Idled` altogether**, on both KWin
  5.27 and sway 1.9. While a video plays, activity reads 100%.
  - Version 2 of the protocol adds `get_input_idle_notification`, which ignores inhibitors.
    Neither compositor here has version 2; Plasma 6 should be checked.
  - KDE's `org.freedesktop.ScreenSaver.GetSessionIdleTime` answers "not supported" on Wayland.
  - Until then, the KDE capability report says that activity can read high while a video
    plays.

## Screenshots (E13-S03)

### GNOME: the Screenshot portal (not recommended)

1. **First call.** The first non-interactive call shows an access dialog, "Allow Applications
   to Take Screenshots?". After Allow, the permission store holds `screenshot`/`screenshot` →
   `'': yes`. For an app the portal can't identify, that single answer covers every unsandboxed
   app.
2. **Later calls.** They needed no dialog and took 71–90 ms each. Each returned the address of a PNG file.
3. **Each capture is visible and leaves a file:**
   - **A flash, a sound and blocked input.** This comes from GNOME Shell 46's screenshot service
     (source extracted from the installed `libshell-14.so`; the portal backend passes
     `flash = true`). The captured area flashes white for 500 ms, input is blocked meanwhile
     (`inhibitEvents: true`), and the "screen-capture" sound plays. The nested capture didn't
     catch the flash; check this on hardware.
   - **A file.** The image goes into the Pictures folder as `Screenshot.png`, `Screenshot-1.png`
     and so on, with mode 0644. Here that was the home folder, because there was no Pictures
     folder. The agent would have to read and delete each one. A crash in between leaves screen
     content in a folder that may be synced or backed up.

While locked, the image is black.

### GNOME: the ScreenCast portal (recommended)

- **First capture.** `persist_mode = 2` with one monitor shows "Share Screen", with "Remember
  this selection" ticked. After Share, the response carries a restore token.
- **Later captures.** With the token, `Start` answered in 9–32 ms with no dialog. That held
  three times in a row, and again in a new GNOME session, because the permission store keeps
  the token on disk.
  - GNOME 46 returned the same token each time. The portal specification makes tokens
    single-use, so the agent always keeps the newest one.
- **Cost.** Pulling one frame through PipeWire took 83–94 ms, including starting
  `gst-launch`, and 1.9 s the first time.
- **No side effects.** No file, no flash and no sound.
- **The indicator.** GNOME shows its orange screen-sharing indicator while the session is open,
  so it blinks briefly at each capture. The UI should say so.
- **What E13-S03 needs:**
  - `libpipewire` through the `pipewire` crate: a C library, like `libdbus`, and already
    installed on GNOME desktops;
  - frames in shared memory (`MemFd`, BGRx), not DMA-BUF;
  - one stream per monitor chosen. With `multiple = true` the member can choose all of them;
    not tested, since there was one monitor. A monitor added later isn't covered and needs a
    new consent;
  - **the token is a capability** ("record this screen without asking"). It is stored encrypted,
    like the device token.

### GNOME: rejected paths

- `org.gnome.Shell.Screenshot` answers "Screenshot is not allowed". Its allowlist holds the
  portal backends, the media keys and `gnome-screenshot`.
- `org.gnome.Mutter.ScreenCast` and `org.gnome.Mutter.RemoteDesktop` accepted `CreateSession`
  from an arbitrary unsandboxed client, because Mutter doesn't check the caller. Using them
  would skip the consent the portal asks for and depend on a private interface. Don't use them.

### KDE

- **Portal.** The portal reported Screenshot version 2 and ScreenCast version 5 with
  xdg-desktop-portal-kde 5.27.11.
- **Screenshot, from the source:**
  - The KDE backend has no `version` property, so portal 1.18 skips its access dialog.
  - A non-interactive screenshot is then taken with no consent, through KWin's older
    `org.kde.kwin.Screenshot`, and saved as `~/Pictures/Screenshot_<date>.png`. KWin authorizes
    the portal through `X-KDE-DBUS-Restricted-Interfaces` in the portal's desktop file.
- **ScreenCast, from the source:** it accepts restore data (outputs or windows) and skips the
  dialog when the data still matches.
- **Not confirmed.** KWin never completed a capture here:
  - on the nested backend, KWin falls back to QPainter, which has no screenshot effect, so the
    portal got no image;
  - on the virtual backend, which uses OpenGL, the portal timed out ("failed to read
    screenshot: timeout"), and so did a direct `ScreenShot2` call.
- **`ScreenShot2` refuses unauthorized callers** ("not authorized to take a screenshot"). An
  installed app can be authorized the same way the portal is, through its desktop file. That
  is an option for the KDE capture story, once capture can be tested on hardware.

### sway

- **`wlr-screencopy` version 3.** Any client may use it, with no consent. A 1280×720 frame took
  4.7 ms with the software renderer.
- **Screenshot portal (xdg-desktop-portal-wlr 0.7.1).** It needs no dialog and took 37 ms, but
  writes to **`/tmp/out.png`, a fixed path with mode 0644.** Any local user can read the last
  screenshot. Don't use it.
- **ScreenCast portal.** It needs a GPU (GBM) and was not tested.

## The app in front (E13-S05, E13-S06)

### GNOME: a Shell extension

Wayland offers nothing here:

- `org.gnome.Shell.Introspect.GetWindows` answers "not allowed";
- `org.gnome.Shell.Eval` is off.

The prototype (below, about 60 lines, GNOME 45 and later) owns `com.worklume.ActiveWindow` and
answers one call:

`com.worklume.ActiveWindow1.Get(with_title) → (app_id, wm_class, pid, title)`

- **Fields:**
  - `wm_class` is the window's Wayland app ID;
  - the PID comes from Mutter;
  - the title is read only when asked;
  - empty values and 0 mean nothing is focused, for example when the overview shows at login.
- **Speed.** A call took 1.3–3.6 ms, measured from Python.
- **Callers are checked.** The executable behind the caller's PID must be on an allowlist, and
  `gdbus` was refused ("Only the Worklume agent may ask"). This is defence in depth: code
  running as the same user can do worse.
- **No signals.** A signal would reach every client on the bus, so the agent polls once a
  second, as on X11.
- **The agent resolves the app.** The Shell's own lookup (`WindowTracker`) returned no
  desktop-file ID, even after a matching entry was added. The agent should resolve the entry
  itself, with the E12-S04 `DesktopFiles` index, from the app ID and the executable, as on X11.
  The extension stays a thin source of facts.
- **It loads at the next login.** An extension installed while the Shell runs isn't loaded
  until the member logs in again, because GNOME can't restart the Shell under Wayland.
  `gnome-extensions enable` succeeded but nothing loaded. After a new session it loaded and
  answered. So the `.deb` installs it system-wide (`/usr/share/gnome-shell/extensions/`) and
  enables it, and the capability report says "after your next login" until it answers.

### KDE: a KWin script or the Plasma window protocol

**KWin script, confirmed.** It is loaded over D-Bus (`/Scripting` `loadScript`, then `start`).
On every activation (`workspace.clientActivated`) it called the agent with
`(resourceClass, desktopFileName, pid)`, and with empty values between windows.

- No polling is needed.
- The script lasts until KWin restarts, so the agent loads it at each start.
- Any session client can load scripts.
- The script calls whoever owns the agent's bus name. The agent must own that name with
  `DO_NOT_QUEUE` and treat losing it as a fault.
- Plasma 6 renames the API (`workspace.windowActivated`, `workspace.activeWindow`). Not tested.

**`org_kde_plasma_window_management`, confirmed.** KWin hides this protocol unless the client's
executable has a desktop file listing it in `X-KDE-Wayland-Interfaces`; the `.deb` can ship one.
With the grant, the probe got:

- each window's app ID, title and PID;
- which window is active.

**Recommendation for E13-S06: the protocol rather than a script.** It needs no bus name, can't
be called by someone else, and is a protocol rather than a scripting API that changed between
5 and 6. Confirm it on Plasma 6.

### sway

`wlr-foreign-toplevel` version 3 reports each window's app ID and title, and whether it is
activated. It has no PID. Confirmed.

## Session lock

- **GNOME.** GNOME Shell called logind's `SetLockedHint(true)` on lock and `SetLockedHint(false)`
  on unlock, and `org.gnome.ScreenSaver.GetActive` answered true. The E12-S03 adapter reads both
  already.
- **KDE.** KWin called `SetLockedHint(true)`, and `org.freedesktop.ScreenSaver.GetActive`
  answered true.
- **sway.** `swaylock` locks through `ext-session-lock` and tells neither logind nor D-Bus. It
  can't be detected.
- **No password problem.** No input events reach the agent on Wayland, so the X11 lock-screen
  password problem doesn't arise. The agent only sees that someone is active.

## App identity

Portal 1.18 identifies an unsandboxed app only from its systemd scope,
`app-<launcher>-<id>-<n>.scope` (source). GNOME starts autostart and launcher apps in such
scopes; an app started from a terminal has none.

- **Without an ID:**
  - dialogs say "An application";
  - the Screenshot permission is shared by every unsandboxed app.
- **With the ID `com.worklume.agent`:**
  - dialogs name Worklume;
  - permissions are the agent's own.

This workspace has no systemd user session, so only the first case was seen. The agent must be
started through its desktop entry; autostart already does this (E12-S05).

## Verdict and proposed backlog changes

| Story                        | Verdict                                                                                                                    |
| ---------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| E13-S02 Detection and report | Go, as above                                                                                                               |
| E13-S03 Portal screenshots   | Go on GNOME, **through the ScreenCast portal and a restore token** rather than the Screenshot portal. KDE is still unknown |
| E13-S04 Idle and activity    | Go on GNOME (`IdleMonitor`). Go on KDE with the video caveat in the capability report                                      |
| E13-S05 GNOME extension      | Go: thin extension, caller allowlist, the agent resolves the app, loads at next login                                      |
| E13-S06 KDE active window    | Go on 5.27 with `org_kde_plasma_window_management`. Plasma 6 unverified                                                    |

Proposed wording changes, for the backlog owner to accept:

- **E13-S03:** "xdg-desktop-portal ScreenCast with a persisted restore token (one consent), one
  frame per capture through PipeWire. The Screenshot portal is not used: on GNOME it flashes,
  plays a sound and writes into Pictures."
- **E13-S06:** "the Plasma window-management protocol, granted through the agent's desktop
  file. A KWin script is the fallback."
- **Capability report (E13-S02):**
  - "activity may read high while a video plays" on KDE and wlroots;
  - "active app available after your next login" on GNOME until the extension answers.

## Crates for E13

- `wayland-client` 0.31 with `wayland-protocols` 0.32 (`staging` for `ext-idle-notify`) and
  `wayland-protocols-plasma`. They are pure Rust and built on the first try.
- `dbus` 0.9, already used, for `IdleMonitor`, the portal and the extension. A portal request
  is a method call answered by a signal on a known path, which a blocking connection can wait
  for, so E13 doesn't need `zbus` (see "Linux sensors" in `docs/conventions/desktop.md`). The
  spike made these calls from Python, not Rust.
- `pipewire` (pipewire-rs) for ScreenCast frames. It links `libpipewire-0.3`.

## Appendix: the prototypes

GNOME Shell extension (`active-window@worklume.com`, `metadata.json` lists shell versions 45–48).
Condensed from the tested copy, which also allowed the spike's Python probe:

```js
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Shell from 'gi://Shell';
import { Extension } from 'resource:///org/gnome/shell/extensions/extension.js';

Gio._promisify(Gio.DBusConnection.prototype, 'call');

const ALLOWED = ['/usr/bin/worklume-desktop'];
const IFACE = `<node><interface name="com.worklume.ActiveWindow1">
  <method name="Get"><arg type="b" direction="in" name="with_title"/>
    <arg type="s" direction="out" name="app_id"/><arg type="s" direction="out" name="wm_class"/>
    <arg type="u" direction="out" name="pid"/><arg type="s" direction="out" name="title"/>
  </method></interface></node>`;

export default class ActiveWindowExtension extends Extension {
  enable() {
    this._impl = Gio.DBusExportedObject.wrapJSObject(IFACE, this);
    this._impl.export(Gio.DBus.session, '/com/worklume/ActiveWindow');
    this._name = Gio.bus_own_name_on_connection(
      Gio.DBus.session,
      'com.worklume.ActiveWindow',
      Gio.BusNameOwnerFlags.NONE,
      null,
      null,
    );
  }

  disable() {
    this._impl.unexport();
    this._impl = null;
    Gio.bus_unown_name(this._name);
  }

  async GetAsync([withTitle], invocation) {
    try {
      const [pid] = (
        await Gio.DBus.session.call(
          'org.freedesktop.DBus',
          '/org/freedesktop/DBus',
          'org.freedesktop.DBus',
          'GetConnectionUnixProcessID',
          new GLib.Variant('(s)', [invocation.get_sender()]),
          null,
          Gio.DBusCallFlags.NONE,
          -1,
          null,
        )
      ).deepUnpack();
      if (!ALLOWED.includes(GLib.file_read_link(`/proc/${pid}/exe`)))
        throw new Error('not the agent');
    } catch (e) {
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
        app && !app.is_window_backed() ? app.get_id() : '',
        window.get_wm_class() ?? '',
        Math.max(window.get_pid(), 0),
        withTitle ? (window.get_title() ?? '') : '',
      ]),
    );
  }
}
```

KWin script (Plasma 5.27 names):

```js
function report(window) {
  callDBus(
    'com.worklume.Agent',
    '/com/worklume/Agent',
    'com.worklume.ActiveWindowSink1',
    'Changed',
    window ? String(window.resourceClass) : '',
    window ? String(window.desktopFileName) : '',
    window ? window.pid : 0,
  );
}
workspace.clientActivated.connect(report);
report(workspace.activeClient);
```
