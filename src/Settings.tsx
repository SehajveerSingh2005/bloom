import { StrictMode, useState, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { Effect } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { check } from "@tauri-apps/plugin-updater";
import { getVersion } from "@tauri-apps/api/app";
import {
  Power,
  Square,
  Maximize2,
  PanelTop,
  BatteryWarning,
  RefreshCw,
  LogOut,
  Palette,
  Droplet,
  Contrast,
  Droplets,
  Sun,
  Monitor,
  Eye,
  EyeOff,
  Calendar,
  Music,
  Minimize2,
  LayoutList,
  Sparkles,
  Circle,
  CloudSun,
  Volume2,
  ArrowLeftToLine,
  ArrowRightToLine,
  Download,
  Settings,
  Hexagon,
  Info,
  X,
} from "lucide-react";
import "./Settings.css";
import { initTheme, hexToHsl } from "./theme";

const appWindow = getCurrentWebviewWindow();

function SettingsApp() {
  useEffect(() => {
    return initTheme();
  }, []);

  const [autostart, setAutostart] = useState(false);
  const [weatherEnabled, setWeatherEnabled] = useState(true);
  const [calendarEnabled, setCalendarEnabled] = useState(true);
  const [musicModeEnabled, setMusicModeEnabled] = useState(true);
  const [musicCompactNotch, setMusicCompactNotch] = useState(true);
  const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(true);
  const [volumeEdgeEnabled, setVolumeEdgeEnabled] = useState(() => localStorage.getItem("bloom-volume-edge-enabled") !== "false");
  const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(true);
  const [brightnessEdgeEnabled, setBrightnessEdgeEnabled] = useState(() => localStorage.getItem("bloom-brightness-edge-enabled") !== "false");
  const [mediaAmbienceEnabled, setMediaAmbienceEnabled] = useState(true);
  const [mediaCompactGlowEnabled, setMediaCompactGlowEnabled] = useState(true);
  const [mediaLayout, setMediaLayout] = useState<'classic' | 'compact'>(() => (localStorage.getItem("bloom-media-layout") as 'classic' | 'compact') || 'classic');
  const [cornersEnabled, setCornersEnabled] = useState(() => localStorage.getItem("bloom-corners-enabled") === "true");
  const [tempUnitFahrenheit, setTempUnitFahrenheit] = useState(false);
  const [cityName, setCityName] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [dockEnabled, setDockEnabled] = useState(true);
  const [dockPreviewEnabled, setDockPreviewEnabled] = useState(true);
  const [dockIconOnly, setDockIconOnly] = useState(() => localStorage.getItem("bloom-dock-icon-only") === "true");
  const [dockMode, setDockMode] = useState(() => localStorage.getItem("bloom-dock-mode") || "auto-hide");
  const [notchMode, setNotchMode] = useState("fixed");
  const [lowBatteryThreshold, setLowBatteryThreshold] = useState(20);
  const [updateStatus, setUpdateStatus] = useState<"idle" | "checking" | "available" | "uptodate" | "error" | "downloading">("idle");
  const [updateVersion, setUpdateVersion] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [autoUpdate, setAutoUpdate] = useState(() => localStorage.getItem("bloom-auto-update") === "true");
  const [scale, setScale] = useState(() => parseFloat(localStorage.getItem("bloom-scale") || "1.0"));
  const [themeMode, setThemeMode] = useState(() => localStorage.getItem("bloom-theme-mode") || "dark");
  const [themeColor, setThemeColor] = useState(() => localStorage.getItem("bloom-theme-color") || "#007aff");
  const [themeOpacity, setThemeOpacity] = useState(() => {
    const val = localStorage.getItem("bloom-theme-opacity");
    return val !== null ? parseFloat(val) : 0.80;
  });
  const [themeSaturation, setThemeSaturation] = useState(() => {
    const val = localStorage.getItem("bloom-theme-saturation");
    return val !== null ? parseFloat(val) : 0.50;
  });
  const [themeBrightness, setThemeBrightness] = useState(() => {
    const val = localStorage.getItem("bloom-theme-brightness");
    return val !== null ? parseFloat(val) : 0.15;
  });

  const [activeTab, setActiveTab] = useState("general");

  useEffect(() => {
    invoke('resize_settings_window', {
      width: 620 * scale,
      height: 480 * scale
    }).catch(console.error);
  }, [scale]);

  // Initialize autostart state and set background effects
  useEffect(() => {
    // Disable context menu
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext as any);

    // ... existing enableBlur ...
    const enableBlur = async () => {
      try {
        await appWindow.setEffects({
          effects: ["mica" as Effect],
          state: "active" as any
        });
      } catch (e) {
      }
    };
    enableBlur();

    async function checkAutostart() {
      try {
        const enabled = await isEnabled();
        setAutostart(enabled);
      } catch (err) {
      }
    }
    checkAutostart();

    // Load all settings
    invoke("load_settings").then((settings: any) => {
      const getVal = (key: string) => {
        const val = settings[key];
        if (val !== undefined && val !== null) return String(val);
        return localStorage.getItem(key);
      };

      const weather = getVal("bloom-weather-enabled");
      if (weather !== null) setWeatherEnabled(weather === "true");

      const calendar = getVal("bloom-calendar-enabled");
      if (calendar !== null) setCalendarEnabled(calendar === "true");

      const musicEnabled = getVal("bloom-music-mode-enabled");
      if (musicEnabled !== null) setMusicModeEnabled(musicEnabled === "true");

      const musicCompact = getVal("bloom-music-compact-notch");
      if (musicCompact !== null) setMusicCompactNotch(musicCompact === "true");

      const volume = getVal("bloom-volume-overlay-enabled");
      if (volume !== null) setVolumeOverlayEnabled(volume === "true");

      const ambience = getVal("bloom-media-ambience-enabled");
      if (ambience !== null) setMediaAmbienceEnabled(ambience === "true");

      const compactGlow = getVal("bloom-media-compact-glow-enabled");
      if (compactGlow !== null) setMediaCompactGlowEnabled(compactGlow === "true");

      const corners = getVal("bloom-corners-enabled");
      if (corners !== null) setCornersEnabled(corners === "true");

      const tempUnit = getVal("bloom-temp-unit");
      if (tempUnit !== null) setTempUnitFahrenheit(tempUnit === "fahrenheit");

      const savedCity = getVal("bloom-weather-city");
      if (savedCity) setCityName(savedCity);

      const dock = getVal("bloom-dock-enabled");
      if (dock !== null) setDockEnabled(dock === "true");

      const dMode = getVal("bloom-dock-mode");
      if (dMode) setDockMode(dMode);

      const threshold = getVal("bloom-low-battery-threshold");
      if (threshold !== null) setLowBatteryThreshold(parseInt(threshold));

      const nMode = getVal("bloom-notch-mode");
      if (nMode) setNotchMode(nMode);

      const preview = getVal("bloom-dock-preview-enabled");
      if (preview !== null) setDockPreviewEnabled(preview === "true");

      const iconOnly = getVal("bloom-dock-icon-only");
      if (iconOnly !== null) setDockIconOnly(iconOnly === "true");

      const scaleVal = getVal("bloom-scale");
      if (scaleVal !== null) setScale(parseFloat(scaleVal));

      const autoUpdateVal = getVal("bloom-auto-update");
      if (autoUpdateVal !== null) setAutoUpdate(autoUpdateVal === "true");

      const tMode = getVal("bloom-theme-mode");
      if (tMode) setThemeMode(tMode);

      const tColor = getVal("bloom-theme-color");
      if (tColor) setThemeColor(tColor);

      const tOpacity = getVal("bloom-theme-opacity");
      if (tOpacity) setThemeOpacity(parseFloat(tOpacity));

      const tSaturation = getVal("bloom-theme-saturation");
      if (tSaturation) setThemeSaturation(parseFloat(tSaturation));

      const tBrightness = getVal("bloom-theme-brightness");
      if (tBrightness) setThemeBrightness(parseFloat(tBrightness));
    }).catch(console.error);

    getVersion().then((ver) => {
      if (ver) {
        setAppVersion(ver);
      } else {
        setAppVersion("3.1.2");
      }
    }).catch((err) => {
      console.error("Failed to load version via Tauri API, using fallback:", err);
      setAppVersion("3.1.2");
    });
    checkForUpdates(false);
  }, []);

  useEffect(() => {
    const unlisten = listen<{ key: string, value: any }>("settings-changed", (event) => {
      const { key, value } = event.payload;
      if (key === "dock-mode") setDockMode(value);
      if (key === "notch-mode") setNotchMode(value);
      if (key === "dock-enabled") setDockEnabled(value);
      if (key === "dock-icon-only") setDockIconOnly(value);
      if (key === "weather") setWeatherEnabled(value);
      if (key === "calendar") setCalendarEnabled(value);
      if (key === "music-mode-enabled") setMusicModeEnabled(value);
      if (key === "music-compact-notch") setMusicCompactNotch(value);
      if (key === "media-ambience-enabled") setMediaAmbienceEnabled(value);
      if (key === "media-compact-glow-enabled") setMediaCompactGlowEnabled(value);
      if (key === "media-layout") setMediaLayout(value as 'classic' | 'compact');
      if (key === "corners-enabled") setCornersEnabled(value);
      if (key === "low-battery-threshold") setLowBatteryThreshold(value);
      if (key === "bloom-scale") setScale(Number(value));
      if (key === "theme-mode") setThemeMode(value);
      if (key === "theme-color") setThemeColor(value);
      if (key === "theme-opacity") setThemeOpacity(Number(value));
      if (key === "theme-saturation") setThemeSaturation(Number(value));
      if (key === "theme-brightness") setThemeBrightness(Number(value));
    });

    const unlistenAccent = listen<string>("system-accent-changed", (event) => {
      const mode = localStorage.getItem("bloom-theme-mode") || "dark";
      if (mode === "adaptive") {
        try {
          const hsl = hexToHsl(event.payload);
          setThemeSaturation(hsl.s / 100);
          setThemeBrightness(hsl.l / 100);
        } catch (e) {
          console.error("Failed to parse system accent color change HSL:", e);
        }
      }
    });

    return () => {
      unlisten.then(fn => fn());
      unlistenAccent.then(fn => fn());
    };
  }, []);




  /* saveAndBroadcast removed because it was unused and causing build errors */

  const checkForUpdates = async (manual = true) => {
    setUpdateStatus("checking");
    try {
      const update = await check();
      if (update) {
        setUpdateStatus("available");
        setUpdateVersion(update.version);
        if (manual) {
          // You could show a prompt or just let the button handle it
        }
      } else {
        setUpdateStatus("uptodate");
      }
    } catch (e) {
      console.error("Updater error:", e);
      setUpdateStatus("error");
    }
  };

  const installUpdate = async () => {
    try {
      const update = await check();
      if (update) {
        setUpdateStatus("downloading");
        await update.downloadAndInstall();
        await invoke("restart_bloom");
      }
    } catch (e) {
      console.error(e);
      setUpdateStatus("error");
    }
  };

  const handleClose = async (e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    try {
      await appWindow.hide();
    } catch(e) {
    }
  };

  const toggleAutostart = async () => {
    try {
      const currentlyEnabled = await isEnabled();
      if (currentlyEnabled) {
        await disable();
        setAutostart(false);
      } else {
        await enable();
        setAutostart(true);
      }
    } catch (err) {
    }
  };

  const notifyChange = (key: string, value: string | boolean | number) => {
    invoke("broadcast_setting", { key, value });
  };

  const saveAndLocal = (key: string, value: string) => {
    localStorage.setItem(key, value);
    invoke("save_setting", { key, value }).catch(console.error);
  };

  const toggleWeather = () => {
    const newVal = !weatherEnabled;
    setWeatherEnabled(newVal);
    saveAndLocal("bloom-weather-enabled", String(newVal));
    notifyChange("weather", newVal);
  };

  const toggleCalendar = () => {
    const newVal = !calendarEnabled;
    setCalendarEnabled(newVal);
    saveAndLocal("bloom-calendar-enabled", String(newVal));
    notifyChange("calendar", newVal);
  };

  const toggleMusicMode = () => {
    const newVal = !musicModeEnabled;
    setMusicModeEnabled(newVal);
    saveAndLocal("bloom-music-mode-enabled", String(newVal));
    notifyChange("music-mode-enabled", newVal);
  };

  const toggleMusicCompactNotch = () => {
    const newVal = !musicCompactNotch;
    setMusicCompactNotch(newVal);
    saveAndLocal("bloom-music-compact-notch", String(newVal));
    notifyChange("music-compact-notch", newVal);
  };

  const toggleMediaLayout = (layout: 'classic' | 'compact') => {
    setMediaLayout(layout);
    saveAndLocal("bloom-media-layout", layout);
    notifyChange("media-layout", layout);
  };

  const toggleVolumeOverlay = () => {
    const newVal = !volumeOverlayEnabled;
    setVolumeOverlayEnabled(newVal);
    saveAndLocal("bloom-volume-overlay-enabled", String(newVal));
    notifyChange("volume-overlay", newVal);
  };

  const toggleVolumeEdge = () => {
    const newVal = !volumeEdgeEnabled;
    setVolumeEdgeEnabled(newVal);
    saveAndLocal("bloom-volume-edge-enabled", String(newVal));
    notifyChange("volume-edge", newVal);
  };

  const toggleBrightnessOverlay = () => {
    const newVal = !brightnessOverlayEnabled;
    setBrightnessOverlayEnabled(newVal);
    saveAndLocal("bloom-brightness-overlay-enabled", String(newVal));
    notifyChange("brightness-overlay", newVal);
  };

  const toggleBrightnessEdge = () => {
    const newVal = !brightnessEdgeEnabled;
    setBrightnessEdgeEnabled(newVal);
    saveAndLocal("bloom-brightness-edge-enabled", String(newVal));
    notifyChange("brightness-edge", newVal);
  };

  const toggleAmbience = () => {
    const newVal = !mediaAmbienceEnabled;
    setMediaAmbienceEnabled(newVal);
    saveAndLocal("bloom-media-ambience-enabled", String(newVal));
    notifyChange("media-ambience-enabled", newVal);
  };

  const toggleCompactGlow = () => {
    const newVal = !mediaCompactGlowEnabled;
    setMediaCompactGlowEnabled(newVal);
    saveAndLocal("bloom-media-compact-glow-enabled", String(newVal));
    notifyChange("media-compact-glow-enabled", newVal);
  };

  const toggleThemeMode = async (mode: string) => {
    setThemeMode(mode);
    saveAndLocal("bloom-theme-mode", mode);
    notifyChange("theme-mode", mode);

    if (mode === 'adaptive') {
      try {
        const accentHex = await invoke<string>('get_system_accent_color');
        const hsl = hexToHsl(accentHex);
        
        setThemeSaturation(hsl.s / 100);
        saveAndLocal("bloom-theme-saturation", String(hsl.s / 100));
        notifyChange("theme-saturation", hsl.s / 100);

        setThemeBrightness(hsl.l / 100);
        saveAndLocal("bloom-theme-brightness", String(hsl.l / 100));
        notifyChange("theme-brightness", hsl.l / 100);
      } catch (e) {
        console.error("Failed to parse adaptive accent HSL:", e);
      }
    }
  };

  const handleThemeColorChange = (color: string) => {
    setThemeColor(color);
    saveAndLocal("bloom-theme-color", color);
    notifyChange("theme-color", color);

    try {
      const hsl = hexToHsl(color);
      
      setThemeSaturation(hsl.s / 100);
      saveAndLocal("bloom-theme-saturation", String(hsl.s / 100));
      notifyChange("theme-saturation", hsl.s / 100);

      setThemeBrightness(hsl.l / 100);
      saveAndLocal("bloom-theme-brightness", String(hsl.l / 100));
      notifyChange("theme-brightness", hsl.l / 100);
    } catch (e) {
      console.error("Failed to parse custom color HSL:", e);
    }
  };

  const handleOpacityChange = (value: number) => {
    setThemeOpacity(value);
    saveAndLocal("bloom-theme-opacity", String(value));
    notifyChange("theme-opacity", value);
  };

  const handleSaturationChange = (value: number) => {
    setThemeSaturation(value);
    saveAndLocal("bloom-theme-saturation", String(value));
    notifyChange("theme-saturation", value);
  };

  const handleBrightnessChange = (value: number) => {
    setThemeBrightness(value);
    saveAndLocal("bloom-theme-brightness", String(value));
    notifyChange("theme-brightness", value);
  };

  const toggleAutoUpdate = () => {
    const newVal = !autoUpdate;
    setAutoUpdate(newVal);
    saveAndLocal("bloom-auto-update", String(newVal));
  };

  const toggleCorners = () => {
    const newVal = !cornersEnabled;
    setCornersEnabled(newVal);
    saveAndLocal("bloom-corners-enabled", String(newVal));
    notifyChange("corners-enabled", newVal);
  };

  const toggleTempUnit = () => {
    const newVal = !tempUnitFahrenheit;
    setTempUnitFahrenheit(newVal);
    saveAndLocal("bloom-temp-unit", newVal ? "fahrenheit" : "celsius");
    notifyChange("temp-unit", newVal);
  };

  const toggleDock = () => {
    const newVal = !dockEnabled;
    setDockEnabled(newVal);
    saveAndLocal("bloom-dock-enabled", String(newVal));
    notifyChange("dock-enabled", newVal);
  };

  const toggleDockPreview = () => {
    const newVal = !dockPreviewEnabled;
    setDockPreviewEnabled(newVal);
    saveAndLocal("bloom-dock-preview-enabled", String(newVal));
    notifyChange("dock-preview-enabled", newVal);
  };

  const toggleDockIconOnly = () => {
    const newVal = !dockIconOnly;
    setDockIconOnly(newVal);
    saveAndLocal("bloom-dock-icon-only", String(newVal));
    notifyChange("dock-icon-only", newVal);
  };

  const toggleDockMode = (newMode: string) => {
    setDockMode(newMode);
    saveAndLocal("bloom-dock-mode", newMode);
    notifyChange("dock-mode", newMode);
  };

  const toggleNotchMode = (newMode: string) => {
    setNotchMode(newMode);
    saveAndLocal("bloom-notch-mode", newMode);
    notifyChange("notch-mode", newMode);
  };

  const handleThresholdChange = (val: number) => {
    setLowBatteryThreshold(val);
    saveAndLocal("bloom-low-battery-threshold", val.toString());
    notifyChange("low-battery-threshold", val);
  };

  const handleScaleChange = (val: number) => {
    setScale(val);
    saveAndLocal("bloom-scale", val.toString());
    notifyChange("bloom-scale", val);
  };


  const handleCityChange = async (newCity: string) => {
    setCityName(newCity);
    if (newCity.trim() === "") {
      localStorage.removeItem("bloom-weather-city");
      localStorage.removeItem("bloom-weather-lat");
      localStorage.removeItem("bloom-weather-lon");
      notifyChange("weather-refresh", true);
      return;
    }

    setIsSearching(true);
    try {
      // Use Open-Meteo Geocoding API (free, no key)
      const res = await fetch(`https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(newCity)}&count=1&language=en&format=json`);
      const data = await res.json();
      if (data.results && data.results.length > 0) {
        const { latitude, longitude, name } = data.results[0];
        saveAndLocal("bloom-weather-city", name);
        saveAndLocal("bloom-weather-lat", latitude.toString());
        saveAndLocal("bloom-weather-lon", longitude.toString());
        setCityName(name);
        notifyChange("weather-refresh", true);
      } else {
        console.warn(`Geocoding: No results found for ${newCity}`);
      }
    } catch (e) {
      console.error("Geocoding failed:", e);
    } finally {
      setIsSearching(false);
    }
  };

  const renderGeneral = () => (
    <>
      <div className="setting-group-label">Startup & Display</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Power size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Launch at Login</span>
            <span className="setting-desc">Open Bloom automatically</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
            <span className="slider"></span>
          </label>
        </div>
        
        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <Square size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Screen Corners</span>
            <span className="setting-desc">Rounded top edges</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={cornersEnabled} onChange={toggleCorners} />
            <span className="slider"></span>
          </label>
        </div>

        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <Maximize2 size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">UI & Font Scale</span>
            <span className="setting-desc">Adjust desktop size (80% - 130%)</span>
          </div>
          <div className="scale-button-container">
            <button 
              onClick={() => handleScaleChange(Math.max(0.8, parseFloat((scale - 0.1).toFixed(1))))}
              disabled={scale <= 0.8}
              className="scale-adjust-btn"
              title="Decrease Scale"
            >
              —
            </button>
            <span className="scale-display-value">{Math.round(scale * 100)}%</span>
            <button 
              onClick={() => handleScaleChange(Math.min(1.3, parseFloat((scale + 0.1).toFixed(1))))}
              disabled={scale >= 1.3}
              className="scale-adjust-btn"
              title="Increase Scale"
            >
              +
            </button>
          </div>
        </div>

        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <PanelTop size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Notch Behavior</span>
            <span className="setting-desc">Auto-hide top bar</span>
          </div>
          <select 
            className="settings-select" 
            value={notchMode} 
            onChange={(e) => toggleNotchMode(e.target.value)}
          >
            <option value="fixed">Fixed</option>
            <option value="auto-hide">Auto Hide</option>
          </select>
        </div>

        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <BatteryWarning size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Low Battery Alert</span>
            <span className="setting-desc">Trigger at {lowBatteryThreshold}%</span>
          </div>
          <input 
            type="range" 
            min="5" 
            max="50" 
            step="5" 
            value={lowBatteryThreshold} 
            onChange={(e) => handleThresholdChange(parseInt(e.target.value))} 
            className="settings-slider"
          />
        </div>
      </div>

      <div className="setting-group-label">Bloom Management</div>
      <div className="setting-group">
        <div className="setting-item action" onClick={() => invoke('restart_bloom')}>
          <div className="setting-icon-bg">
            <RefreshCw size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Restart Bloom</span>
            <span className="setting-desc">Reinitialize all components</span>
          </div>
        </div>
        
        <div className="setting-divider" />

        <div className="setting-item action danger" onClick={() => invoke('quit_bloom')}>
          <div className="setting-icon-bg">
            <LogOut size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Quit Bloom</span>
            <span className="setting-desc">Exit application completely</span>
          </div>
        </div>
      </div>
    </>
  );

  const renderAppearance = () => (
    <>
      <div className="setting-group-label">Appearance & Theme</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Palette size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Theme Mode</span>
            <span className="setting-desc">Configure visual styling</span>
          </div>
          <select 
            className="settings-select" 
            value={themeMode} 
            onChange={(e) => toggleThemeMode(e.target.value)}
          >
            <option value="dark">Dark (Translucent)</option>
            <option value="light">Light (Translucent)</option>
            <option value="custom">Custom Color</option>
            <option value="adaptive">Adaptive Accent</option>
          </select>
        </div>

        {themeMode === 'custom' && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Droplet size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Custom Theme Color</span>
                <span className="setting-desc">Choose layout background color</span>
              </div>
              <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
                <input 
                  type="color" 
                  value={themeColor} 
                  onChange={(e) => handleThemeColorChange(e.target.value)}
                  style={{
                    border: 'none',
                    width: '28px',
                    height: '28px',
                    borderRadius: '50%',
                    cursor: 'pointer',
                    padding: 0,
                    background: 'transparent',
                    overflow: 'hidden'
                  }}
                />
                <span style={{ fontSize: '11px', fontWeight: 600, color: 'var(--text-muted)' }}>
                  {themeColor.toUpperCase()}
                </span>
              </div>
            </div>
          </>
        )}

        <div className="setting-divider" />
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Contrast size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Background Opacity</span>
            <span className="setting-desc">Adjust theme transparency ({Math.round(themeOpacity * 100)}%)</span>
          </div>
          <input 
            type="range" 
            min="0.1" 
            max="1.0" 
            step="0.05" 
            value={themeOpacity} 
            onChange={(e) => handleOpacityChange(parseFloat(e.target.value))} 
            className="settings-slider"
          />
        </div>

        {(themeMode === 'custom' || themeMode === 'adaptive') && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Droplets size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Color Saturation</span>
                <span className="setting-desc">Adjust theme color vibrancy ({Math.round(themeSaturation * 100)}%)</span>
              </div>
              <input 
                type="range" 
                min="0.0" 
                max="1.0" 
                step="0.02" 
                value={themeSaturation} 
                onChange={(e) => handleSaturationChange(parseFloat(e.target.value))} 
                className="settings-slider"
              />
            </div>

            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Sun size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Background Brightness</span>
                <span className="setting-desc">Adjust background lightness ({Math.round(themeBrightness * 100)}%)</span>
              </div>
              <input 
                type="range" 
                min="0.0" 
                max="1.0" 
                step="0.02" 
                value={themeBrightness} 
                onChange={(e) => handleBrightnessChange(parseFloat(e.target.value))} 
                className="settings-slider"
              />
            </div>
          </>
        )}
      </div>

      <div className="setting-group-label">Bloom Dock</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Monitor size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Bloom Dock</span>
            <span className="setting-desc">Replace Windows taskbar</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={dockEnabled} onChange={toggleDock} />
            <span className="slider"></span>
          </label>
        </div>

        {dockEnabled && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                {dockMode === 'fixed' ? <EyeOff size={14} strokeWidth={1.5} /> : <Eye size={14} strokeWidth={1.5} />}
              </div>
              <div className="setting-info">
                <span className="setting-label">Behavior</span>
                <span className="setting-desc">{dockMode === 'fixed' ? 'Reserves screen space' : 'Auto hide when not in use'}</span>
              </div>
              <select 
                className="settings-select" 
                value={dockMode} 
                onChange={(e) => toggleDockMode(e.target.value)}
              >
                <option value="fixed">Fixed</option>
                <option value="auto-hide">Auto Hide</option>
              </select>
            </div>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Eye size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Show App Previews</span>
                <span className="setting-desc">Show window thumbnails on hover</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={dockPreviewEnabled} onChange={toggleDockPreview} />
                <span className="slider"></span>
              </label>
            </div>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Circle size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Icon Only</span>
                <span className="setting-desc">Remove icon background and padding</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={dockIconOnly} onChange={toggleDockIconOnly} />
                <span className="slider"></span>
              </label>
            </div>
          </>
        )}
      </div>
    </>
  );

  const renderModules = () => (
    <>
      <div className="setting-group-label">Notch</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Calendar size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Calendar & Timer</span>
            <span className="setting-desc">Enable productivity split-view</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={calendarEnabled} onChange={toggleCalendar} />
            <span className="slider"></span>
          </label>
        </div>

        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <Music size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Music Mode</span>
            <span className="setting-desc">Interactive live music widget</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={musicModeEnabled} onChange={toggleMusicMode} />
            <span className="slider"></span>
          </label>
        </div>

        {musicModeEnabled && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Minimize2 size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Compact Mode</span>
                <span className="setting-desc">Show visualizer & artwork when collapsed</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={musicCompactNotch} onChange={toggleMusicCompactNotch} />
                <span className="slider"></span>
              </label>
            </div>

            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <LayoutList size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Media Layout</span>
                <span className="setting-desc">Choose expanded player style</span>
              </div>
              <div className="unit-toggle-minimal wide">
                <span className={mediaLayout === 'classic' ? 'active' : ''} onClick={() => toggleMediaLayout('classic')}>Classic</span>
                <span className={mediaLayout === 'compact' ? 'active' : ''} onClick={() => toggleMediaLayout('compact')}>Compact</span>
              </div>
            </div>

            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Sparkles size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Ambient Glow</span>
                <span className="setting-desc">Colored glow behind expanded album art</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={mediaAmbienceEnabled} onChange={toggleAmbience} />
                <span className="slider"></span>
              </label>
            </div>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <Circle size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Compact Glow</span>
                <span className="setting-desc">Glow around collapsed thumbnail</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={mediaCompactGlowEnabled} onChange={toggleCompactGlow} />
                <span className="slider"></span>
              </label>
            </div>
          </>
        )}
      </div>

      <div className="setting-group-label">Weather</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <CloudSun size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Weather Status</span>
            <span className="setting-desc">Passive temperature info</span>
          </div>
          <div className="weather-controls">
            <div className="unit-toggle-minimal" onClick={toggleTempUnit}>
              <span className={!tempUnitFahrenheit ? "active" : ""}>C</span>
              <span className={tempUnitFahrenheit ? "active" : ""}>F</span>
            </div>
            <label className="toggle-switch">
              <input type="checkbox" checked={weatherEnabled} onChange={toggleWeather} />
              <span className="slider"></span>
            </label>
          </div>
        </div>

        {weatherEnabled && (
          <>
            <div className="setting-divider" />
            <div className="manual-city-input">
              <input
                type="text"
                placeholder="Enter city manually..."
                value={cityName}
                onChange={(e) => setCityName(e.target.value)}
                onBlur={() => handleCityChange(cityName)}
                onKeyDown={(e) => e.key === 'Enter' && handleCityChange(cityName)}
              />
              {isSearching && <div className="searching-spinner" />}
            </div>
          </>
        )}
      </div>

      <div className="setting-group-label">Overlays</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Volume2 size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Volume HUD</span>
            <span className="setting-desc">Bloom volume overlay</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={volumeOverlayEnabled} onChange={toggleVolumeOverlay} />
            <span className="slider"></span>
          </label>
        </div>

        {volumeOverlayEnabled && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <ArrowLeftToLine size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Show on Edge Hover</span>
                <span className="setting-desc">Slide in from left edge</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={volumeEdgeEnabled} onChange={toggleVolumeEdge} />
                <span className="slider"></span>
              </label>
            </div>
          </>
        )}

        <div className="setting-divider" />

        <div className="setting-item">
          <div className="setting-icon-bg">
            <Sun size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Brightness HUD</span>
            <span className="setting-desc">Bloom brightness overlay</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={brightnessOverlayEnabled} onChange={toggleBrightnessOverlay} />
            <span className="slider"></span>
          </label>
        </div>

        {brightnessOverlayEnabled && (
          <>
            <div className="setting-divider" />
            <div className="setting-item">
              <div className="setting-icon-bg">
                <ArrowRightToLine size={14} strokeWidth={1.5} />
              </div>
              <div className="setting-info">
                <span className="setting-label">Show on Edge Hover</span>
                <span className="setting-desc">Slide in from right edge</span>
              </div>
              <label className="toggle-switch">
                <input type="checkbox" checked={brightnessEdgeEnabled} onChange={toggleBrightnessEdge} />
                <span className="slider"></span>
              </label>
            </div>
          </>
        )}
      </div>
    </>
  );

  const renderAbout = () => (
    <div className="about-tab-container">
      <div className="about-header">
        <img src="/bloom.png" className="about-logo" alt="Bloom Logo" />
        <h1 className="about-title">Bloom</h1>
        <p className="about-version">Version {appVersion}</p>
      </div>

      <div className="setting-group-label" style={{ marginTop: '24px' }}>Software Updates</div>
      <div className="setting-group">
        <div className="setting-item">
          <div className="setting-icon-bg">
            <Download size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">Auto Update</span>
            <span className="setting-desc">Update automatically on startup</span>
          </div>
          <label className="toggle-switch">
            <input type="checkbox" checked={autoUpdate} onChange={toggleAutoUpdate} />
            <span className="slider"></span>
          </label>
        </div>

        <div className="setting-divider" />

        <div className="setting-item action" onClick={() => updateStatus === 'available' ? installUpdate() : checkForUpdates()}>
          <div className="setting-icon-bg">
            <RefreshCw size={14} strokeWidth={1.5} />
          </div>
          <div className="setting-info">
            <span className="setting-label">
              {updateStatus === 'idle' && "Check for Updates"}
              {updateStatus === 'checking' && "Checking..."}
              {updateStatus === 'available' && `Update Available (v${updateVersion})`}
              {updateStatus === 'uptodate' && "Bloom is up to date"}
              {updateStatus === 'downloading' && "Downloading Update..."}
              {updateStatus === 'error' && "No updates found"}
            </span>
            <span className="setting-desc">
              {updateStatus === 'available' ? "Click to install and restart" : `Currently running v${appVersion}`}
            </span>
          </div>
        </div>
      </div>

      <div className="about-footer">
        <p>Made with ❤️ by sehaz</p>
      </div>
    </div>
  );

  return (
    <div className="settings-container" style={{ zoom: scale }}>
      <div className="title-bar" data-tauri-drag-region>
        <span className="title-text" data-tauri-drag-region>Settings</span>
        <button className="close-btn" onClick={handleClose} title="Close Settings">
          <X size={12} strokeWidth={1.5} style={{ pointerEvents: 'none' }} />
        </button>
      </div>

      <div className="settings-body">
        <div className="settings-sidebar">
          <button 
            className={`sidebar-tab ${activeTab === 'general' ? 'active' : ''}`}
            onClick={() => setActiveTab('general')}
          >
            <div className="sidebar-tab-icon">
              <Settings size={14} strokeWidth={1.5} />
            </div>
            <span>General</span>
          </button>
          <button 
            className={`sidebar-tab ${activeTab === 'appearance' ? 'active' : ''}`}
            onClick={() => setActiveTab('appearance')}
          >
            <div className="sidebar-tab-icon">
              <Palette size={14} strokeWidth={1.5} />
            </div>
            <span>Appearance</span>
          </button>
          <button 
            className={`sidebar-tab ${activeTab === 'modules' ? 'active' : ''}`}
            onClick={() => setActiveTab('modules')}
          >
            <div className="sidebar-tab-icon">
              <Hexagon size={14} strokeWidth={1.5} />
            </div>
            <span>Modules</span>
          </button>
          <button 
            className={`sidebar-tab ${activeTab === 'about' ? 'active' : ''}`}
            onClick={() => setActiveTab('about')}
          >
            <div className="sidebar-tab-icon">
              <Info size={14} strokeWidth={1.5} />
            </div>
            <span>About</span>
          </button>
        </div>

        <div className="settings-content">
          {activeTab === 'general' && renderGeneral()}
          {activeTab === 'appearance' && renderAppearance()}
          {activeTab === 'modules' && renderModules()}
          {activeTab === 'about' && renderAbout()}
        </div>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root") as HTMLElement).render(
  <StrictMode>
    <SettingsApp />
  </StrictMode>
);
