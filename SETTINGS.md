# Bloom Settings Reference

All settings are stored in `settings.json` in the app config directory (`%APPDATA%/bloom/`). The file is a flat JSON object with `bloom-` prefixed keys. Bloom watches this file for external changes and applies them in real-time.

## Quick Start

Edit `settings.json` with any text editor while Bloom is running. Changes are applied immediately — no restart required.

```json
{
  "bloom-dock-enabled": "true",
  "bloom-dock-mode": "smart",
  "bloom-theme-mode": "dark",
  "bloom-scale": "1.0"
}
```

## Settings Keys

### Dock

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-dock-enabled` | `"true"` / `"false"` | `"true"` | Show or hide the Bloom Dock (taskbar replacement). |
| `bloom-dock-mode` | `"fixed"` / `"smart"` / `"peek"` | `"fixed"` | Dock visibility behavior. **fixed** = always visible as AppBar. **smart** = auto-hide when overlapped by fullscreen apps. **peek** = hidden until cursor approaches bottom edge. |
| `bloom-dock-preview-enabled` | `"true"` / `"false"` | `"true"` | Show window thumbnail previews when hovering dock icons. |
| `bloom-dock-icon-only` | `"true"` / `"false"` | `"false"` | Minimal icon-only style (no background/padding around icons). |

### Notch

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-notch-mode` | `"fixed"` / `"smart"` / `"peek"` | `"fixed"` | Notch (top bar) visibility behavior. Same modes as dock. **peek** shows the notch briefly on media events and notifications. |

### Weather

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-weather-enabled` | `"true"` / `"false"` | `"true"` | Show weather widget in the notch status bar. |
| `bloom-weather-city` | string | `""` | Manually set city name for weather. Empty string = auto-detect via IP geolocation. |
| `bloom-weather-lat` | number string | (auto) | Latitude coordinate for weather. Set automatically when a city is selected. |
| `bloom-weather-lon` | number string | (auto) | Longitude coordinate for weather. Set automatically when a city is selected. |
| `bloom-weather-cached-temp` | number string | (none) | Cached temperature value shown before next API fetch. |
| `bloom-weather-cached-condition` | string | (none) | Cached weather condition text (e.g. "Partly Cloudy"). |
| `bloom-temp-unit` | `"celsius"` / `"fahrenheit"` | `"celsius"` | Temperature display unit. |

### Modules

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-calendar-enabled` | `"true"` / `"false"` | `"true"` | Enable calendar/timer mode in the notch. |
| `bloom-music-mode-enabled` | `"true"` / `"false"` | `"true"` | Enable interactive music media widget. |
| `bloom-music-compact-notch` | `"true"` / `"false"` | `"true"` | Show compact music display (visualizer + artwork) in collapsed notch. |

### Music Appearance

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-media-layout` | `"classic"` / `"compact"` | `"classic"` | Expanded player style. **classic** = large album art. **compact** = small thumbnail + controls. |
| `bloom-media-ambience-enabled` | `"true"` / `"false"` | `"true"` | Colored ambient glow behind expanded album art. |
| `bloom-media-compact-glow-enabled` | `"true"` / `"false"` | `"true"` | Glow effect around the collapsed compact thumbnail. |
| `bloom-media-visualizer-enabled` | `"true"` / `"false"` | `"true"` | Audio visualizer bars in music mode. Also accepts `bloom-visualizer-enabled` (legacy alias). |
| `bloom-media-album-art-enabled` | `"true"` / `"false"` | `"true"` | Show album artwork in the notch music display. |

### Overlays

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-volume-overlay-enabled` | `"true"` / `"false"` | `"true"` | Show Bloom volume HUD when volume changes (replaces native Windows OSD). |
| `bloom-volume-edge-enabled` | `"true"` / `"false"` | `"true"` | Trigger volume HUD by hovering the left screen edge. |
| `bloom-brightness-overlay-enabled` | `"true"` / `"false"` | `"true"` | Show Bloom brightness HUD when brightness changes. |
| `bloom-brightness-edge-enabled` | `"true"` / `"false"` | `"true"` | Trigger brightness HUD by hovering the right screen edge. |

### Appearance

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-theme-mode` | `"dark"` / `"light"` / `"custom"` / `"adaptive"` | `"dark"` | Theme mode. **dark** = dark translucent. **light** = light translucent. **custom** = user-picked color. **adaptive** = follows Windows system accent color. |
| `bloom-theme-color` | hex string | `"#007aff"` | Custom theme color (used in `custom` and `adaptive` modes). |
| `bloom-theme-opacity` | float string | `"0.80"` | Background opacity (0.1 to 1.0). |
| `bloom-theme-saturation` | float string | `"0.50"` | Color saturation for custom/adaptive themes (0.0 to 1.0). |
| `bloom-theme-brightness` | float string | `"0.15"` | Background brightness for custom/adaptive themes (0.0 to 1.0). |
| `bloom-corners-enabled` | `"true"` / `"false"` | `"false"` | Render rounded screen corner overlays on top edges. |

### Status Widgets

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-status-widgets` | JSON string | `{"left":["weather"],"right":["battery"]}` | Widget layout in collapsed notch. Available: `"weather"`, `"battery"`, `"cpu"`, `"ram"`, `"disk"`, `"net"`. |

Example:
```json
{
  "bloom-status-widgets": "{\"left\":[\"cpu\",\"ram\"],\"right\":[\"battery\",\"net\"]}"
}
```

### System

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| `bloom-scale` | float string | `"1.0"` | UI scale factor (0.8 to 1.3). Changing this re-registers AppBars to resize the reserved screen area. |
| `bloom-low-battery-threshold` | integer string | `"20"` | Battery percentage that triggers the low-battery alert pulse (5 to 50, step 5). |
| `bloom-auto-update` | `"true"` / `"false"` | `"false"` | Check for and download updates automatically on startup. |
| `bloom-show-update-indicator` | `"true"` / `"false"` | `"true"` | Show a green dot on the notch when an update is available. |

### Internal (Do Not Edit Manually)

| Key | Type | Description |
|-----|------|-------------|
| `bloom-first-run` | sentinel | Set to `"done"` after first launch. Triggers splash screen if absent. |
| `bloom-app-version` | string | Last known app version. If it differs from current, splash screen is shown on update. |

## Event System

Bloom uses two Tauri events for settings synchronization:

### `settings-changed`
- **Emitted by:** `save_setting` command (frontend or backend)
- **Payload:** `{ "key": "bloom-...", "value": ... }`
- **Purpose:** Broadcasts changes made through the Bloom UI to all windows
- **Key format:** Bloom-prefixed keys as-is (e.g. `"bloom-dock-mode"`)

### `settings-external-changed`
- **Emitted by:** File watcher (detects external edits to `settings.json`)
- **Payload:** `{ "key": "bloom-...", "value": ... }` or `{ "key": "bloom-...", "value": null }` for removed keys
- **Purpose:** Broadcasts changes made by external editors (VS Code, notepad, scripts)
- **Key format:** Bloom-prefixed keys as-is
- **Behavior:** Also syncs values to `localStorage` for instant frontend reads

### Flow
```
External editor saves settings.json
    ↓
File watcher detects change (ReadDirectoryChangesW)
    ↓
Diffs against SETTINGS_CACHE
    ↓
Emits settings-external-changed for each changed/removed key
    ↓
useSettingsSync hook updates React state + localStorage
    ↓
useSettingsSync hook updates React state
    ↓
UI re-renders with new values
```

## Example: Changing Dock Mode via Script

```powershell
# PowerShell: switch dock to smart mode
$json = Get-Content "$env:APPDATA\bloom\settings.json" | ConvertFrom-Json
$json.'bloom-dock-mode' = 'smart'
$json | ConvertTo-Json | Set-Content "$env:APPDATA\bloom\settings.json"
```

```python
# Python: disable dock
import json, os
path = os.path.join(os.environ['APPDATA'], 'bloom', 'settings.json')
with open(path) as f: settings = json.load(f)
settings['bloom-dock-enabled'] = 'false'
with open(path, 'w') as f: json.dump(settings, f)
```

## Notes

- All boolean values are strings (`"true"` / `"false"`) for consistency with `localStorage`.
- The `useSettingsSync` hook auto-converts `"true"` / `"false"` strings to booleans.
- `auto-hide` mode values in `bloom-dock-mode` and `bloom-notch-mode` are legacy aliases for `smart` — they are mapped automatically.
- Changing `bloom-scale` triggers AppBar re-registration to adjust reserved screen space.
- Theme changes (`bloom-theme-*`) are applied by reading all theme values from `localStorage` and calling `applyTheme()` — the theme system depends on all five theme keys being in sync.
