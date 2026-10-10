# Dock background apps and system actions

An unpinned app remains in the dock after its last window closes only if Bloom
observed that app with a window during the current session and it still has a
live notification icon. Other tray-only apps do not fill the dock. Pinned apps
show a grey dot from startup when they are already active in the tray. Windowed
apps retain their normal window dots and previews.
Running unpinned apps appear to the right of the pinned group. Settings > Dock >
App Separator controls the thin theme-aware divider; it is on by default and
appears only when an unpinned app is present.

- Click a background app to invoke its notification icon's default action.
- Right-click a background app to request its native tray context menu.
- Shift-right-click opens Bloom's menu for pinning and icon customization.
- An app with several tray icons has a separate menu entry for each icon.
- Start's context menu includes Task Manager, Disk Management, Device Manager,
  Computer Management and Windows settings shortcuts. The dock background menu
  includes Task Manager and Taskbar Settings.

## Implementation and compatibility

`tray.rs` reads notification-icon candidates from Windows 11's
`HKCU\Control Panel\NotifyIconSettings`. Each candidate must match a running
process path and pass `Shell_NotifyIconGetRect`; historical entries alone never
count as running. Explorer-hosted system icons (including Bluetooth) are excluded
because their process path is shared with File Explorer windows. The frontend reconciles tray owners with actual window handles,
including applications launched through shell IDs. A shared browser tray does
not combine multiple PWAs. Pinned shell IDs also match visible app windows when
their executable paths differ, so opening Bloom's Settings window does not add
an unpinned duplicate of a pinned Bloom app.

Native interactions use UI Automation. Windows reports the chevron rectangle for
an icon hidden in the overflow, so hidden icons are matched by the live
`UIOrderList` order against Explorer's overflow buttons, with equal-count and
nonempty-label checks. Any label that uniquely identifies another icon must
also agree with that order. App-defined tooltip text may differ from the
executable description, so it cannot be required to contain the process name.
Promoted icons are matched to the
taskbar's XAML controls by screen position, accounting for display scaling.
Failure produces a user-visible error. Menu opening temporarily exposes the
native tray and restores the taskbar on completion or error, without changing
AppBar state or icon promotion preferences. Windows' connection/transaction
timeouts bound UIA provider calls.

The registry layout and Explorer's UIA tree are Windows implementation details.
Windows versions or alternative shells that do not expose them keep the existing
window-based dock behavior. Windows 10 tray discovery is not implemented here.
Background processes without notification icons are not included.

API references:

- [Shell_NotifyIconGetRect](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/nf-shellapi-shell_notifyicongetrect)
- [IUIAutomationElement3::ShowContextMenu](https://learn.microsoft.com/en-us/windows/win32/api/uiautomationclient/nf-uiautomationclient-iuiautomationelement3-showcontextmenu)

## Validation

Automated checks:

```powershell
bun run build
bun test scripts/dockApps.test.ts
cd src-tauri
cargo check --locked
cargo clippy --locked --all-targets
cargo test --locked
# Optional read-only integration check against the current desktop:
cargo test --locked live_tray_discovery -- --ignored --nocapture
```

Manual acceptance checks (required before merging):

1. On Bloom startup, unrelated tray-only apps stay out of the dock; pinned apps
   already active in the tray show a grey dot.
2. Start an unpinned Discord with one window: one normal dot, no duplicate entry.
3. Close Discord to the tray: one grey dot, no window thumbnail.
4. Right-click its dock icon: Discord's own menu appears and its actions work.
5. Click the background icon: Discord responds through its tray default action.
6. Quit Discord through its native menu: unpinned icon disappears; pinned dot clears
   within the 10-second safety poll.
7. Repeat with an icon in overflow, a promoted icon, and an app with multiple icons.
   Include apps whose tooltips differ from their executable names, such as
   Tailscale, Phone Link, and Windows Security.
8. Exit/restart Explorer, exit Bloom during menu opening, and toggle the dock off:
   the taskbar must return to the intended state with no transparent residue.
9. Check Start and dock system actions, including denied UAC and launch errors.
10. Check light/dark themes, large Bloom scale, multiple monitors and mixed DPI.
11. Toggle App Separator off and on: only the divider changes; unpinned apps stay
    on the right. Open pinned Bloom and check that Settings appears without a
    second unpinned Bloom icon. File Explorer must not show Bluetooth tray actions.

Live discovery, the Discord background dot, and Discord's native context menu
have been exercised on the development machine. The broader compatibility,
menu-action, and restoration checks above still require end-to-end validation.
