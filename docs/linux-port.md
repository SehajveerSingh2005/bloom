# Linux port audit and roadmap

## Current architecture

Bloom is a Tauri 2 desktop application with a React/Vite frontend. Four
transparent webviews (`main`, `dock`, `overlay`, and `settings`) share a Rust
backend through Tauri commands. The frontend owns animation, settings UI, and
layout; the backend owns desktop integration. The current backend is almost
entirely Windows-specific: `commands.rs` exposes the IPC API, `services.rs`
runs hooks/workers, and `utils.rs` implements Shell, taskbar, icon, and DWM
helpers. `state.rs` holds cross-worker state. This makes it the correct seam
for a platform layer rather than a frontend rewrite.

The Linux baseline adds `platform/linux/`, which owns every desktop integration:

| Module | Responsibility |
| --- | --- |
| `apps.rs` | XDG `.desktop` discovery, the entry index used to attribute windows to applications, and shell-free launching |
| `audio.rs` | Volume and mute through `wpctl`/`pactl`; the visualizer's monitor capture and FFT |
| `icons.rs` | Freedesktop icon theme lookup, including XSETTINGS theme detection, rendered as data URIs |
| `bluetooth.rs` | Bluetooth power and settings through BlueZ over system D-Bus |
| `media.rs` | MPRIS transport, metadata and artwork over session D-Bus |
| `network.rs` | Wi-Fi radio state and settings through NetworkManager |
| `power.rs` | Battery state and power profile from UPower over system D-Bus |
| `system.rs` | CPU, RAM, disk and network metrics via `sysinfo`; internal-display brightness via `/sys/class/backlight` |
| `thumbnails.rs` | Read-only X11 window captures for dock previews |
| `windows_manager.rs` | EWMH window enumeration, focus/close, focus timestamps, and the `windows-changed` watcher |
| `x11.rs` | One shared X11 connection, cached atoms, property reads, EWMH client messages and XSETTINGS parsing |

`commands_linux.rs` preserves the existing command names. Features without a
native Linux implementation report unavailability through their existing
`Result` contract instead of pretending to work.

## Windows dependency and Linux replacement map

| Existing implementation | Bloom use | Linux Mint Cinnamon/X11 replacement | Difficulty |
| --- | --- | --- | --- |
| Win32 Shell/COM (`IShellLink`, `ShellExecute`, shell icon APIs) | Start-menu discovery, `.lnk` resolution, launch, icons | XDG `.desktop` parsing, icon theme lookup, `Command` launch | Medium |
| `SHAppBarMessage`, `Shell_TrayWnd`, taskbar hooks | Replace/hide Windows taskbar and reserve edges | Cinnamon-specific panel integration, initially coexist safely | Hard |
| WinEvent hooks, HWND, `EnumWindows`, DWM | Window list, focus/close, change events, work areas | X11/EWMH via `x11rb`; XComposite/XDamage for previews | Hard |
| WASAPI loopback, endpoint volume | Visualizer and volume/mute | PipeWire (`pipewire` crate), PulseAudio compatibility | Hard |
| GSMTC / Windows media controls | Media metadata and transport | D-Bus MPRIS | Medium |
| WMI/Power Manager/registry | Brightness, battery saver, system data, accent color | `/sys/class/backlight`, UPower D-Bus, `sysinfo`, Cinnamon settings | Medium |
| Windows Radios APIs | Wi-Fi and Bluetooth toggles | NetworkManager D-Bus and BlueZ D-Bus | Medium |
| Windows low-level keyboard/mouse hooks | Media/brightness keys and cursor behavior | XInput2 only for the required explicit events | Hard |
| IP Helper APIs | Network throughput | `sysinfo` or netlink counters | Medium |

## Recommended module shape

```text
src-tauri/src/platform/
  mod.rs
  windows/                 # progressively extract existing backend code
    apps.rs audio.rs media.rs windows_manager.rs panel.rs system.rs
  linux/
    mod.rs
    apps.rs                # implemented in the baseline
    audio.rs media.rs windows_manager.rs panel.rs system.rs
    network.rs bluetooth.rs brightness.rs
```

The command modules remain IPC adapters: they validate inputs, call a platform
service, and translate a normal capability error to the stable frontend type.

## Dependency plan

`x11rb` (EWMH/X11) and `sysinfo` (metrics) are in use, plus `zbus` for MPRIS,
UPower, PowerProfiles, NetworkManager and BlueZ. `zbus` was already in the tree
via Tauri, so it adds no new transitive dependencies. `rustfft`, `base64` and
`image` were already shared with the Windows backend and are now used on Linux
too. Mint build prerequisites are WebKitGTK, GTK3, AppIndicator, and librsvg.
At runtime the desktop integrations expect `wpctl` (or `pactl`) and `pw-record`
(or `parec`) from the default Mint audio stack, and `cinnamon-settings` (with
`nm-connection-editor` / `blueman-manager` as fallbacks) for the settings
buttons. Each feature degrades to a reported unavailability when its tool or
service is missing, so none of these are hard requirements for launching Bloom.

## Delivery order

1. ~~Launch UI and preserve IPC on Linux (baseline).~~ Done.
2. ~~Finish XDG apps: icon-theme resolution and cached discovery.~~ Done.
3. ~~Metrics, brightness and UPower battery state.~~ Done.
4. ~~Volume, mute and the audio visualizer.~~ Done.
5. ~~MPRIS media metadata and transport.~~ Done.
6. ~~Add NetworkManager (Wi-Fi) and BlueZ (Bluetooth).~~ Done.
7. ~~Add X11 EWMH window management and events.~~ Done.
8. ~~Build a Cinnamon-safe dock that coexists with the panel; defer panel
   replacement until it is explicitly designed and recovery-tested.~~ Done.
9. ~~Window previews.~~ Done.
10. ~~Screen-edge hover and covered-window tracking, so the dock's smart mode
    and the notch's fullscreen hiding actually work.~~ Done.
11. ~~Desktop accent colour, custom application icons, settings panels, and the
    splash/OSD overlay.~~ Done.
12. ~~Linux packages.~~ A `.deb` is built and verified; AppImage needs the Tauri
    AppImage tooling downloaded at build time.
13. Remaining: global input handling is deliberately not implemented (see
    limitations), and Wayland is out of scope for this milestone.

## Implemented and verified

Verified against a live Mint/Cinnamon X11 session with real applications:

* Window enumeration finds the managed windows, skips support windows, Bloom's
own windows, `_NET_WM_STATE_SKIP_TASKBAR` windows, and anything whose
  `_NET_WM_WINDOW_TYPE` is not normal or dialog.
* Windows are attributed to `.desktop` entries through `StartupWMClass`, the
  desktop file name, then the process executable, so the dock groups them the
  same way the Windows backend does. Windows of one application collapse into a
  single dock item with every window in `all_hwnds`.
* `focus_window` sends `_NET_ACTIVE_WINDOW`, or `WM_CHANGE_STATE` when the
  window is already active, matching the Windows focus/minimize toggle.
  `close_window` sends `_NET_CLOSE_WINDOW`. These messages name the window they
  concern and are delivered to the root window, which are two different fields:
  addressing them at the root is accepted by the window manager and then ignored,
  which is what previously left the dock unable to switch between windows.
  `focus_and_minimize_toggle` exercises both against a live desktop on demand
  (`cargo test --release -- --ignored`).
* Icons resolve through the desktop\'s real theme with XSETTINGS
  (`Net/IconThemeName`), then GTK settings, then the other installed themes, then
  `hicolor`. Both the `<size>/<category>` and `<category>/<size>` theme layouts
  are supported, and symbolic and cursor-only themes are excluded.
* A `windows-changed` watcher emits only when the window list or active window
  actually changes, using two property reads per tick.
* Click-through uses the pointer position and only sends input-shape requests
  after a window has been realized, because sending them earlier crashes inside
  tao on GTK.

* **Audio control** goes through `wpctl` (PipeWire/WirePlumber) with `pactl`
  (PulseAudio) as the fallback, because neither libpipewire nor libpulse
  development headers can be assumed at build time. These are one-shot calls on
  user actions, never in the visualizer's hot path.
* **Volume changes are detected from events, not a poll.** `pactl subscribe`
  streams an event the moment a sink changes, so the on-screen display appears as
  soon as a volume key is pressed — measured at roughly 200 ms, where polling
  once a second meant it could lag by up to a second, or be missed entirely for a
  change above 100%. A one-second poll remains as the fallback where the
  PulseAudio layer is unavailable, and the subscription restarts if it drops.
* **Display brightness** is *set* through logind's `SetBrightness`, which is how
  the desktop's own brightness keys apply it. The kernel device node is
  root-owned, so writing it directly fails for the user even though the desktop
  succeeds; logind performs the write for the active session, authorised by
  polkit. The direct write is kept as a fallback for systems that grant access
  through a group or a udev rule.
* **Brightness changes are reported** from a 120 ms poll of the kernel backlight,
  which is a single small sysfs read with no subprocess involved. Nothing else
  announces brightness on Linux, so before this Bloom's brightness display never
  appeared at all. A change Bloom makes itself is not echoed back to it, so
  dragging its own brightness slider does not pop the display.
* **The visualizer** streams the default sink's monitor with `pw-record`
  (`-P stream.capture.sink=true`) and runs the same 512-point FFT and five band
  weights as the Windows backend. Capture only runs while MPRIS reports playback,
  so an idle desktop costs nothing. `parec` is only a fallback: it delivers no
  audio on at least one tested PipeWire setup, so `pw-record` is preferred.
* **Media control** uses MPRIS, so it works with any MPRIS-capable player
  (Firefox, Chromium, Spotify, mpv, VLC, and others). Metadata is emitted on
  every change, and position only on changes greater than one second, mirroring
  the Windows backend's throttle.
* **Battery** comes from UPower. The frontend's Battery Status API path is kept
  for Windows because WebKitGTK does not implement `navigator.getBattery`.
* **Wi-Fi** reads NetworkManager's `WirelessEnabled` software radio state and
  toggles it through the same property. **Bluetooth** discovers the adapter
  through BlueZ's `ObjectManager` rather than assuming `hci0`, and toggles
  `Powered`. Both report authorization failures rather than failing silently, and
  Bloom never requests root.
* **Window previews** capture the window drawable directly with `GetImage`. A
  compositing window manager (Muffin on Cinnamon) keeps every window's contents
  in offscreen storage, so the capture is that window's own pixels. It is a
  read-only operation: Bloom never redirects windows, so a failed capture cannot
  break the desktop. Captures are cached for 1.5 s and ordered by focus time.
* **Screen-edge hover** is reported over the top, bottom, left and right eight
  pixel borders of the primary monitor, each with the same 500 ms linger the
  Windows mouse hook uses, so a revealed surface is not closed while the pointer
  travels onto it. The pointer position is polled once for both this and
  click-through, and only the position is read.
* **The left and right edges reveal the volume and brightness displays on
  Windows, but not on Linux by default.** On Windows the wheel adjusts the level
  while the pointer rests on the edge, so the display has something to do there.
  Linux has no such wheel handling and the desktop already shows its own
  indicator when the level changes, so Bloom keeps its display for real changes
  only — the keyboard keys and Bloom's own volume and brightness controls. The
  `bloom-volume-edge-enabled` and `bloom-brightness-edge-enabled` settings still
  turn the edge reveal back on, and both the top and bottom edges keep working
  as before.
* **Covered-window tracking** reads `_NET_ACTIVE_WINDOW` plus `_NET_WM_STATE` and
  the active window's root-relative geometry, then reports `dock-overlap`,
  `notch-overlap` and `visibility-change`. A fullscreen window is recognised from
  its EWMH state rather than inferred from geometry, and Bloom's own windows, the
  desktop, panels and splash screens are ignored. Verified against a live window
  over the dock band, over the notch band, beside the dock, maximized, and
  fullscreen.
* **The overlay window** covers the primary monitor, so its left and right
  notches sit against the real screen edges. It doubles as the splash screen and
  as the volume and brightness on-screen displays; a real volume change was
  observed taking it from unmapped to viewable at the monitor's full size.
* **Custom application icons** are stored as PNGs under
  `~/.config/bloom/custom_icons` with a JSON manifest mapping flattened
  filenames back to the frontend's keys, mirroring the Windows layout. A saved
  icon always takes precedence over the theme's when resolving an icon, and keys
  containing path separators are flattened so they cannot escape the directory.
* **The `.deb`** installs `/usr/bin/bloom`, a hicolor icon set and a desktop
  entry that was checked with `desktop-file-validate`. The entry carries a
  `Utility` category, a display name, a comment and keywords, because Tauri's
  default left `Categories=` empty and the name lowercase. Package dependencies
  resolve to `libwebkit2gtk-4.1-0`, `libgtk-3-0` and
  `libayatana-appindicator3-1`.
* **Settings buttons** open the matching Cinnamon Settings panel
  (`sound`, `power`, `network`, `notifications`, `panel`), with the Bluetooth
  manager as a fallback where relevant.
* On Linux, media keys are handled by the desktop's own volume control rather
  than by a global input hook. Cinnamon performs the adjustment and Bloom
  reflects it through the `volume-change` event, so both on-screen displays
  appear; Bloom does not capture or suppress the key itself.

## Known limitations

* **Album artwork** is only inlined when the player reports a `file://` art URL,
  because the webview's CSP allows `data:` images only and Bloom will not fetch
  remote artwork. Players that expose `https://` art URLs show no album art.
* **Minimized windows have no preview**, because Bloom will not un-minimize
  another application's window to capture it. The dock shows its label instead.
* **Fully transparent (ARGB) windows** capture as a flat image. Their stored
  colour is undefined where alpha is zero, so there is nothing meaningful to
  capture; this affects compositing-only windows such as another dock.
* **Wi-Fi and Bluetooth state is read when the UI loads**, matching the Windows
  backend. Changes made from Cinnamon's own controls are picked up on the next
  load rather than pushed.
* **Volume is reported at the sink's real level**, so a desktop amplified above
  100%, which both PipeWire and PulseAudio allow and laptops commonly use, reads
  as e.g. 120%. The progress bars saturate at full while the figure beside them
  shows the real value, and Bloom never amplifies past 100% itself: its own
  slider still offers 0-100%. This differs from the Windows backend, which caps
  at 100% because the Windows audio API does.
* **Brightness is expressed as a percentage**, as on Windows, while the kernel
  works in raw device units. A round trip through Bloom can therefore land a few
  raw units away from where it started (e.g. 1367 to 1363 out of 1515), which is
  not visible.
* **The desktop accent colour** is read from Cinnamon's `accent-color` key, then
  the XApp portal's `accent-rgb`, then GNOME's `accent-color`. On tested Mint
  22.x with Cinnamon 6.6 none of those keys exist — accents there are baked into
  theme variants such as `Jasper-Blue` — so `get_system_accent_color` returns an
  error and Bloom's adaptive theme keeps its previous colours. The frontend
  already handles that failure. Nothing is invented as a substitute.
* **`open_notification_center` and `open_system_tray`** open Cinnamon's
  notification and panel settings. Cinnamon has no system-level notification
  centre or overflow tray to reveal, so these are the closest real surfaces
  rather than a Windows-equivalent action.
* **Restart** re-executes the process through Tauri's restart. Unlike Windows,
  Linux Bloom registers no AppBars and hides no panel, so no desktop state needs
  unwinding first.
* **Minimized windows are not un-minimized by Bloom** to produce a preview, and
  Bloom never modifies another application's window state.
* **Airplane mode** has no Cinnamon equivalent, so `open_airplane_mode_settings`
  opens the network panel. Bloom never blocks radios itself, because that needs
  root.
* **Set Position** is sent as an MPRIS `SetPosition` with the track object path,
  so seeking requires the player to report `mpris:trackid` and `CanSeek`.
* **Window titles are not watched.** `windows-changed` fires on window list and
  focus changes, so a dock label can lag behind a title-only change.
* **Focus stealing prevention** may refuse `_NET_ACTIVE_WINDOW` because Bloom has
  no event timestamp to pass; it identifies itself as a pager, which most window
  managers accept.
* **Startup notification** (`StartupWMClass` may be absent) and Electron/PWA host
  processes can fall back to a window-title-derived dock item.
* Wayland is not supported yet. `x11.rs` is the only module that assumes X11, so
  a Wayland backend can replace it without touching the command layer.

## First milestone status

The first milestone is complete and was exercised on a live Mint/Cinnamon X11
session: the React UI bootstraps, Windows dependencies are target-scoped, and
app discovery, launching, icons, window management, metrics, audio, media,
radios, battery, window previews, screen-edge hover and covered-window tracking
are all native Linux implementations. It does not hide or modify the Cinnamon
panel, and it registers no desktop state that would need restoring after a
crash.

On Linux the dock and notch windows are anchored to the primary monitor's bottom
and top edges rather than reserved as AppBars, so they are correct at any
resolution while reserving nothing from the desktop. The dock's fixed,
auto-hide and peek modes are computed in the frontend from the now-emitted
overlap events.

The commands that remain without a real Linux backend are `hide_native_osd`,
which hides the Windows volume OSD (there is no native one to hide), and the
global input hook.

The dock's and notch's `fixed`, `smart` and `peek` modes are computed in the
frontend from the overlap events described above, so all three work. On Linux
they default to `smart`, so a freshly installed Bloom gets out of the way as
soon as a window covers it, the way a Linux dock or top bar is expected to
behave. Windows keeps `fixed`. Either default only applies until the user picks
a mode in Bloom's own interface, after which the saved choice wins.

A minimized window is not treated as covering anything. It stays
`_NET_ACTIVE_WINDOW` and keeps the geometry it had before it was hidden, so
without an explicit on-screen check the dock would remain tucked away on an
empty desktop. Both `_NET_WM_STATE_HIDDEN` and the window's map state are
checked, because Muffin marks an iconified window hidden while leaving the
client window mapped. Initial support is Linux Mint 22.x Cinnamon on X11;
Wayland is not supported yet.
