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

| Key                             | Type                             | Default   | Description                                                                                                                                                                      |
| ------------------------------- | -------------------------------- | --------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bloom-dock-enabled`            | `"true"` / `"false"`             | `"true"`  | Show or hide the Bloom Dock (taskbar replacement).                                                                                                                               |
| `bloom-dock-mode`               | `"fixed"` / `"smart"` / `"peek"` | `"smart"` | Dock visibility behavior. **fixed** = always visible as AppBar. **smart** = auto-hide when overlapped by fullscreen apps. **peek** = hidden until cursor approaches bottom edge. |
| `bloom-dock-preview-enabled`    | `"true"` / `"false"`             | `"true"`  | Show window thumbnail previews when hovering dock icons.                                                                                                                         |
| `bloom-dock-icon-only`          | `"true"` / `"false"`             | `"false"` | Minimal icon-only style (no background/padding around icons).                                                                                                                    |
| `bloom-start-icon`              | icon key / `custom:<data URI>`   | `"default"` | Start button icon: `default`, `bloom-colorful`, `bloom-golden`, `bloom-biscuit`, `windows`, or `custom:` followed by an uploaded image's data URI.                               |
| `bloom-dock-adaptive`           | `"true"` / `"false"`             | `"false"` | Fixed dock only. Stretch the dock to full width like a traditional taskbar while a window is maximized, and contract back when it's restored.                                    |
| `bloom-dock-win-number-enabled` | `"true"` / `"false"`             | `"true"`  | When the taskbar is replaced, Win+1 through Win+9 activate the matching pinned dock app (focus/minimize if running, launch otherwise) instead of the native taskbar slots.       |

### Notch

| Key                      | Type                             | Default   | Description                                                                                                                  |
| ------------------------ | -------------------------------- | --------- | ---------------------------------------------------------------------------------------------------------------------------- |
| `bloom-notch-mode`       | `"fixed"` / `"smart"` / `"peek"` | `"fixed"` | Notch (top bar) visibility behavior. Same modes as dock. **peek** shows the notch briefly on media events and notifications. |
| `bloom-notch-edge-delay` | milliseconds `"0"`–`"2000"`      | `"200"`   | In smart/peek mode, how long the cursor must rest against the top screen edge before the notch peeks. `"0"` reveals instantly. |

### Weather

| Key                              | Type                         | Default     | Description                                                                        |
| -------------------------------- | ---------------------------- | ----------- | ---------------------------------------------------------------------------------- |
| `bloom-weather-enabled`          | `"true"` / `"false"`         | `"true"`    | Show weather widget in the notch status bar.                                       |
| `bloom-weather-city`             | string                       | `""`        | Manually set city name for weather. Empty string = auto-detect via IP geolocation. |
| `bloom-weather-lat`              | number string                | (auto)      | Latitude coordinate for weather. Set automatically when a city is selected.        |
| `bloom-weather-lon`              | number string                | (auto)      | Longitude coordinate for weather. Set automatically when a city is selected.       |
| `bloom-weather-cached-temp`      | number string                | (none)      | Cached temperature value shown before next API fetch.                              |
| `bloom-weather-cached-condition` | string                       | (none)      | Cached weather condition text (e.g. "Partly Cloudy").                              |
| `bloom-temp-unit`                | `"celsius"` / `"fahrenheit"` | `"celsius"` | Temperature display unit.                                                          |

### Modules

| Key                         | Type                 | Default  | Description                                                           |
| --------------------------- | -------------------- | -------- | --------------------------------------------------------------------- |
| `bloom-calendar-enabled`    | `"true"` / `"false"` | `"true"` | Enable calendar/timer mode in the notch.                              |
| `bloom-info-centre`        | `"true"` / `"false"` | `"false"` | Merge the notch into the dock as one info centre ("Merge with Dock"). |
| `bloom-glass`               | `"true"` / `"false"` | `"true"`  | Frosted glass (native blur) under the dock and notch; off saves power. |
| `bloom-music-mode-enabled`  | `"true"` / `"false"` | `"true"` | Enable interactive music media widget.                                |
| `bloom-music-compact-notch` | `"true"` / `"false"` | `"true"` | Show compact music display (visualizer + artwork) in collapsed notch. |

### Music Appearance

| Key                                | Type                      | Default     | Description                                                                                     |
| ---------------------------------- | ------------------------- | ----------- | ----------------------------------------------------------------------------------------------- |
| `bloom-media-layout`               | `"classic"` / `"compact"` | `"classic"` | Expanded player style. **classic** = large album art. **compact** = small thumbnail + controls. |
| `bloom-media-ambience-enabled`     | `"true"` / `"false"`      | `"true"`    | Colored ambient glow behind expanded album art.                                                 |
| `bloom-media-compact-glow-enabled` | `"true"` / `"false"`      | `"true"`    | Glow effect around the collapsed compact thumbnail.                                             |
| `bloom-media-visualizer-enabled`   | `"true"` / `"false"`      | `"true"`    | Audio visualizer bars in music mode. Also accepts `bloom-visualizer-enabled` (legacy alias).    |
| `bloom-media-album-art-enabled`    | `"true"` / `"false"`      | `"true"`    | Show album artwork in the notch music display.                                                  |

### Overlays

| Key                                | Type                 | Default  | Description                                                              |
| ---------------------------------- | -------------------- | -------- | ------------------------------------------------------------------------ |
| `bloom-volume-overlay-enabled`     | `"true"` / `"false"` | `"true"` | Show Bloom volume HUD when volume changes (replaces native Windows OSD). |
| `bloom-volume-edge-enabled`        | `"true"` / `"false"` | `"true"` | Trigger volume HUD by hovering the left screen edge.                     |
| `bloom-brightness-overlay-enabled` | `"true"` / `"false"` | `"true"` | Show Bloom brightness HUD when brightness changes.                       |
| `bloom-brightness-edge-enabled`    | `"true"` / `"false"` | `"true"` | Trigger brightness HUD by hovering the right screen edge.                |

### AI

Bloom AI is optional and off by default. Its agent (`bloom-ai.exe`) lives in `%LOCALAPPDATA%\com.sehaz.bloom\ai\` and starts on first use. API keys and email passwords are kept in Windows Credential Manager (service `bloom-ai`), never in this file. "Delete AI altogether" removes the agent, its credentials, its data and every key below, and writes `ai_deleted.flag` next to this file so it is never reinstalled.

| Key                  | Type                                                 | Default                     | Description                                                                 |
| -------------------- | ---------------------------------------------------- | --------------------------- | --------------------------------------------------------------------------- |
| `bloom-ai-enabled`   | `"true"` / `"false"`                                 | `"false"`                   | Turn Bloom AI on. Off stops the agent at once and frees the hotkey.         |
| `bloom-ai-base-url`  | URL                                                  | `"https://api.openai.com/v1"` | Any OpenAI-compatible chat endpoint.                                      |
| `bloom-ai-model`     | string                                               | `""`                        | Model id. Required.                                                         |
| `bloom-ai-stt-url`   | URL                                                  | same as `bloom-ai-base-url` | OpenAI-compatible transcription endpoint (cloud or a local Whisper server). |
| `bloom-ai-stt-model` | string                                               | `"whisper-1"`               | Transcription model id.                                                     |
| `bloom-ai-hotkey`    | virtual-key code                                     | `"165"` (Right Alt)         | Hold to record, release to send. The key no longer reaches apps while AI is on. |
| `bloom-ai-name`      | string (1-24 letters, spaces, hyphens, apostrophes)  | `"Janice"`                  | What the assistant is called; the wake phrase is "Hey <name>". Renaming needs the wake word retrained (`ai\wake
ame.txt` records the trained name); until then the wake word stays off. |
| `bloom-ai-wake`      | `"true"` / `"false"`                                 | `"false"`                   | "Hey <name>" wake word. While on, the microphone is open and the wake word is checked locally; only the request said after it is sent for transcription. Needs training first (`ai\wake\`). |
| `bloom-ai-whatsapp`  | `"true"` / `"false"`                                 | `"false"`                   | "Connect WhatsApp": links the agent as a device on your WhatsApp (unofficial; WhatsApp may restrict accounts it flags). While on, the agent keeps running with one connection open. The session keys are in `ai\whatsapp\`; "Unlink" logs out and deletes them. Messages are kept in memory only. |
| `bloom-ai-whatsapp-selfchat` | `"true"` / `"false"`                          | `"false"`                   | "Answer me in my own chat" (shown once linked): a message you write in your own WhatsApp chat (Message yourself) that starts with the assistant's name ("Janice, ...") runs as a normal request with all tools and your security level, and the reply goes back to that chat. Anything that needs your OK shows on the PC and is asked in the chat; there only "yes" (any case) within 5 minutes approves, any other message cancels; a question too long for one message is answered on the PC. At most 30 requests per hour, one at a time; "Janice, stop" stops the running one. Off: messages in your own chat are ignored. Each request is logged in `actions.log` (`whatsapp-request`) without its text. |
| `bloom-ai-whatsapp-autoreply` | `"true"` / `"false"`                       | `"false"`                   | "Reply automatically" (shown once linked): the agent answers chosen contacts on its own with a separate model call that has no tools, memory, skills or contacts. Waits 15 s after the last message, shows "typing" for 2-8 s, then sends; at most 1 reply per chat per 2 minutes and 30 per hour; pauses a chat for 30 minutes when you write there; stops a chat after 5 replies in 10 minutes until you write there. Never groups. |
| `bloom-ai-whatsapp-auto` | JSON array of numbers, or `"*"`                  | `"[]"`                      | Who gets automatic replies: E.164 numbers from `ai\phones.json`, or `*` for anyone saved there. |
| `bloom-ai-whatsapp-style` | text                                            | `"Let them know I'll get back to them soon. Be brief and friendly."` | "How to reply", in your words. |
| `bloom-ai-whatsapp-sign` | `"true"` / `"false"`                             | `"true"`                    | "Say it's Janice": automatic replies end with "(Janice, &lt;your first name&gt;'s assistant)". |
| `bloom-ai-security`  | `"conservative"` / `"competent"` / `"carte-blanche"` | `"conservative"`            | When the agent asks before sending email or running PowerShell.            |
| `bloom-ai-email`     | address                                              | `""`                        | Account the agent sends email from.                                         |
| `bloom-ai-smtp-host` | host                                                 | `""`                        | Mail server for providers Bloom has no preset for. Empty uses the preset for your address. |
| `bloom-ai-smtp-port` | number                                               | `""`                        | Port for `bloom-ai-smtp-host` (465 TLS, or 587 STARTTLS). Empty uses the preset port. |
| `bloom-ai-debug`     | `"true"` / `"false"`                                 | `"false"`                   | Troubleshooting log: the agent appends wake word scores (detections, near misses and failed rebuilds), what you asked, each tool call with its arguments, tool results (first 300 characters) and replies to `ai\debug.log`, one JSON line each. Past 512 KB it becomes `debug.old.log`, so the two stay around 1 MB. API keys and mail passwords are never written; request text, tool arguments (such as email bodies) and tool results are. Off writes nothing. Takes effect on the next request or wake word. |

### Appearance

| Key                      | Type                                             | Default     | Description                                                                                                                                                 |
| ------------------------ | ------------------------------------------------ | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `bloom-theme-mode`       | `"dark"` / `"light"` / `"custom"` / `"adaptive"` | `"dark"`    | Theme mode. **dark** = dark translucent. **light** = light translucent. **custom** = user-picked color. **adaptive** = follows Windows system accent color. |
| `bloom-theme-color`      | hex string                                       | `"#007aff"` | Custom theme color (used in `custom` and `adaptive` modes).                                                                                                 |
| `bloom-theme-opacity`    | float string                                     | `"0.80"`    | Background opacity (0.1 to 1.0).                                                                                                                            |
| `bloom-theme-saturation` | float string                                     | `"0.50"`    | Color saturation for custom/adaptive themes (0.0 to 1.0).                                                                                                   |
| `bloom-theme-brightness` | float string                                     | `"0.15"`    | Background brightness for custom/adaptive themes (0.0 to 1.0).                                                                                              |
| `bloom-corners-enabled`  | `"true"` / `"false"`                             | `"false"`   | Render rounded screen corner overlays on top edges.                                                                                                         |

### Status Widgets

| Key                    | Type        | Default                                    | Description                                                                                                 |
| ---------------------- | ----------- | ------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| `bloom-status-widgets` | JSON string | `{"left":["weather"],"right":["battery"]}` | Widget layout in collapsed notch. Available: `"weather"`, `"battery"`, `"cpu"`, `"ram"`, `"disk"`, `"net"`. |

Example:

```json
{
	"bloom-status-widgets": "{\"left\":[\"cpu\",\"ram\"],\"right\":[\"battery\",\"net\"]}"
}
```

### System

| Key                           | Type                 | Default   | Description                                                                                          |
| ----------------------------- | -------------------- | --------- | ---------------------------------------------------------------------------------------------------- |
| `bloom-scale`                 | float string         | `"1.0"`   | UI scale factor (0.8 to 1.3). Changing this re-registers AppBars to resize the reserved screen area. |
| `bloom-low-battery-threshold` | integer string       | `"20"`    | Battery percentage that triggers the low-battery alert pulse (5 to 50, step 5).                      |
| `bloom-auto-update`           | `"true"` / `"false"` | `"false"` | Check for and download updates automatically on startup.                                             |
| `bloom-show-update-indicator` | `"true"` / `"false"` | `"true"`  | Show a green dot on the notch when an update is available.                                           |
| `bloom-announcement-dismissed` | announcement id      | `""`      | ID of the last dismissed in-app announcement (published via `website/public/announcements.json`).    |
| `bloom-time-format-24h`       | `"true"` / `"false"` | `"false"` | Use 24-hour clock format in the notch. When `"false"`, displays 12-hour format with AM/PM.           |

### Internal (Do Not Edit Manually)

| Key                 | Type     | Description                                                                           |
| ------------------- | -------- | ------------------------------------------------------------------------------------- |
| `bloom-first-run`   | sentinel | Set to `"done"` after first launch. Triggers splash screen if absent.                 |
| `bloom-app-version` | string   | Last known app version. If it differs from current, splash screen is shown on update. |

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
- Settings → General → Reset to Defaults removes every `bloom-` key from `settings.json` and `localStorage` (except the `bloom-first-run` / `bloom-app-version` lifecycle sentinels), then restarts Bloom. Pinned apps (`pinned_apps.json`) and uploaded icons (`custom_icons/`) are preserved.
- Theme changes (`bloom-theme-*`) are applied by reading all theme values from `localStorage` and calling `applyTheme()` — the theme system depends on all five theme keys being in sync.
