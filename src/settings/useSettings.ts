import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { getVersion } from "@tauri-apps/api/app";
import type { UpdateCheckResult } from "../updater";
import { useSettingsSync } from "../hooks/useSettingsSync";
import { hexToHsl } from "../theme";
import type { WidgetConfig } from "./types";

function saveSetting(key: string, value: string) {
  localStorage.setItem(key, value);
  invoke("save_setting", { key, value }).catch(console.error);
}

function readBool(val: string | null): boolean {
  return val === "true";
}

export function useSettings() {
  const [autostart, setAutostart] = useState(false);
  const [weatherEnabled, setWeatherEnabled] = useState(true);
  const [calendarEnabled, setCalendarEnabled] = useState(true);
  const [musicModeEnabled, setMusicModeEnabled] = useState(true);
  const [musicCompactNotch, setMusicCompactNotch] = useState(true);
  const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(true);
  const [volumeEdgeEnabled, setVolumeEdgeEnabled] = useState(
    () => localStorage.getItem("bloom-volume-edge-enabled") !== "false"
  );
  const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(
    () => localStorage.getItem("bloom-brightness-overlay-enabled") !== "false"
  );
  const [brightnessEdgeEnabled, setBrightnessEdgeEnabled] = useState(
    () => localStorage.getItem("bloom-brightness-edge-enabled") !== "false"
  );
  const [mediaAmbienceEnabled, setMediaAmbienceEnabled] = useState(true);
  const [mediaCompactGlowEnabled, setMediaCompactGlowEnabled] = useState(true);
  const [mediaLayout, setMediaLayout] = useState<"classic" | "compact">(
    () => (localStorage.getItem("bloom-media-layout") as "classic" | "compact") || "classic"
  );
  const [cornersEnabled, setCornersEnabled] = useState(
    () => localStorage.getItem("bloom-corners-enabled") === "true"
  );
  const [showUpdateIndicator, setShowUpdateIndicator] = useState(
    () => localStorage.getItem("bloom-show-update-indicator") !== "false"
  );
  const [timeFormat24h, setTimeFormat24h] = useState(
    () => localStorage.getItem("bloom-time-format-24h") === "true"
  );
  const [tempUnitFahrenheit, setTempUnitFahrenheit] = useState(false);
  const [cityName, setCityName] = useState("");
  const [citySearchResults, setCitySearchResults] = useState<
    Array<{ name: string; country: string; latitude: number; longitude: number }>
  >([]);
  const [showCityDropdown, setShowCityDropdown] = useState(false);
  const [statusWidgets, setStatusWidgets] = useState<WidgetConfig>({
    left: ["weather"],
    right: ["battery"],
  });
  const [dockEnabled, setDockEnabled] = useState(true);
  const [dockPreviewEnabled, setDockPreviewEnabled] = useState(true);
  const [dockIconOnly, setDockIconOnly] = useState(
    () => localStorage.getItem("bloom-dock-icon-only") === "true"
  );
  const [dockAdaptive, setDockAdaptive] = useState(
    () => localStorage.getItem("bloom-dock-adaptive") === "true"
  );
  const [dockMode, setDockMode] = useState(() => {
    const raw = localStorage.getItem("bloom-dock-mode") || "smart";
    return raw === "auto-hide" ? "smart" : raw;
  });
  const [notchMode, setNotchMode] = useState("fixed");
  const [lowBatteryThreshold, setLowBatteryThreshold] = useState(20);
  const [updateStatus, setUpdateStatus] = useState<
    "idle" | "checking" | "available" | "uptodate" | "error" | "downloading" | "installing"
  >("idle");
  const [updateVersion, setUpdateVersion] = useState("");
  const [appVersion, setAppVersion] = useState("");
  const [autoUpdate, setAutoUpdate] = useState(
    () => localStorage.getItem("bloom-auto-update") === "true"
  );
  const [scale, setScale] = useState(
    () => parseFloat(localStorage.getItem("bloom-scale") || "1.0")
  );
  const [themeMode, setThemeMode] = useState(
    () => localStorage.getItem("bloom-theme-mode") || "dark"
  );
  const [themeColor, setThemeColor] = useState(
    () => localStorage.getItem("bloom-theme-color") || "#007aff"
  );
  const [themeOpacity, setThemeOpacity] = useState(() => {
    const val = localStorage.getItem("bloom-theme-opacity");
    return val !== null ? parseFloat(val) : 0.8;
  });
  const [themeSaturation, setThemeSaturation] = useState(() => {
    const val = localStorage.getItem("bloom-theme-saturation");
    return val !== null ? parseFloat(val) : 0.5;
  });
  const [themeBrightness, setThemeBrightness] = useState(() => {
    const val = localStorage.getItem("bloom-theme-brightness");
    return val !== null ? parseFloat(val) : 0.15;
  });
  const [exportStatus, setExportStatus] = useState<"idle" | "exporting" | "success" | "error">("idle");
  const [importStatus, setImportStatus] = useState<"idle" | "importing" | "success" | "error">("idle");

  // ── Load all settings from backend + localStorage ──
  const loadAllSettings = useCallback(async () => {
    try {
      const settings: Record<string, string> = await invoke("load_settings");
      const getVal = (key: string) => {
        const val = settings[key];
        if (val !== undefined && val !== null) return String(val);
        return localStorage.getItem(key);
      };

      const apply = <T>(val: string | null, setter: (v: T) => void, transform: (v: string) => T) => {
        if (val !== null) setter(transform(val));
      };

      apply(getVal("bloom-weather-enabled"), setWeatherEnabled, readBool);
      apply(getVal("bloom-calendar-enabled"), setCalendarEnabled, readBool);
      apply(getVal("bloom-music-mode-enabled"), setMusicModeEnabled, readBool);
      apply(getVal("bloom-music-compact-notch"), setMusicCompactNotch, readBool);
      apply(getVal("bloom-volume-overlay-enabled"), setVolumeOverlayEnabled, readBool);
      apply(getVal("bloom-brightness-overlay-enabled"), setBrightnessOverlayEnabled, readBool);
      apply(getVal("bloom-media-ambience-enabled"), setMediaAmbienceEnabled, readBool);
      apply(getVal("bloom-media-compact-glow-enabled"), setMediaCompactGlowEnabled, readBool);
      apply(getVal("bloom-corners-enabled"), setCornersEnabled, readBool);
      apply(getVal("bloom-time-format-24h"), setTimeFormat24h, readBool);
      apply(getVal("bloom-show-update-indicator"), setShowUpdateIndicator, readBool);
      apply(getVal("bloom-auto-update"), setAutoUpdate, readBool);
      apply(getVal("bloom-volume-edge-enabled"), setVolumeEdgeEnabled, readBool);
      apply(getVal("bloom-brightness-edge-enabled"), setBrightnessEdgeEnabled, readBool);
      apply(getVal("bloom-dock-enabled"), setDockEnabled, readBool);
      apply(getVal("bloom-dock-preview-enabled"), setDockPreviewEnabled, readBool);
      apply(getVal("bloom-dock-icon-only"), setDockIconOnly, readBool);
      apply(getVal("bloom-dock-adaptive"), setDockAdaptive, readBool);

      apply(getVal("bloom-temp-unit"), setTempUnitFahrenheit, (v) => v === "fahrenheit");
      apply(getVal("bloom-scale"), setScale, parseFloat);
      apply(getVal("bloom-low-battery-threshold"), setLowBatteryThreshold, parseInt);

      apply(getVal("bloom-notch-mode"), setNotchMode, (v) => (v === "auto-hide" ? "smart" : v));
      apply(getVal("bloom-dock-mode"), setDockMode, (v) => (v === "auto-hide" ? "smart" : v));

      const savedCity = getVal("bloom-weather-city");
      if (savedCity) setCityName(savedCity);

      apply(getVal("bloom-theme-mode"), setThemeMode, (v) => v);
      apply(getVal("bloom-theme-color"), setThemeColor, (v) => v);
      apply(getVal("bloom-theme-opacity"), setThemeOpacity, parseFloat);
      apply(getVal("bloom-theme-saturation"), setThemeSaturation, parseFloat);
      apply(getVal("bloom-theme-brightness"), setThemeBrightness, parseFloat);

      const widgetsVal = getVal("bloom-status-widgets");
      if (widgetsVal) {
        try {
          const parsed = JSON.parse(widgetsVal);
          if (parsed && Array.isArray(parsed.left) && Array.isArray(parsed.right)) {
            setStatusWidgets(parsed);
          }
        } catch {}
      }
    } catch (e) {
      console.error("Failed to load settings:", e);
    }
  }, []);

  // ── Initialize on mount ──
  useEffect(() => {
    loadAllSettings();

    isEnabled()
      .then(setAutostart)
      .catch(() => {});

    getVersion()
      .then((ver) => setAppVersion(ver || "3.1.2"))
      .catch(() => setAppVersion("3.1.2"));

    checkForUpdates(false);
  }, []);

  // ── Sync settings from other windows ──
  useSettingsSync({
    "bloom-dock-mode": setDockMode,
    "bloom-notch-mode": setNotchMode,
    "bloom-dock-enabled": setDockEnabled,
    "bloom-dock-icon-only": setDockIconOnly,
    "bloom-dock-preview-enabled": setDockPreviewEnabled,
    "bloom-dock-adaptive": setDockAdaptive,
    "bloom-weather-enabled": setWeatherEnabled,
    "bloom-calendar-enabled": setCalendarEnabled,
    "bloom-music-mode-enabled": setMusicModeEnabled,
    "bloom-music-compact-notch": setMusicCompactNotch,
    "bloom-media-ambience-enabled": setMediaAmbienceEnabled,
    "bloom-media-compact-glow-enabled": setMediaCompactGlowEnabled,
    "bloom-media-layout": setMediaLayout,
    "bloom-corners-enabled": setCornersEnabled,
    "bloom-show-update-indicator": setShowUpdateIndicator,
    "bloom-time-format-24h": setTimeFormat24h,
    "bloom-low-battery-threshold": setLowBatteryThreshold,
    "bloom-scale": setScale,
    "bloom-temp-unit": (v) => setTempUnitFahrenheit(v === "fahrenheit"),
    "bloom-auto-update": setAutoUpdate,
    "bloom-volume-overlay-enabled": setVolumeOverlayEnabled,
    "bloom-volume-edge-enabled": setVolumeEdgeEnabled,
    "bloom-brightness-overlay-enabled": setBrightnessOverlayEnabled,
    "bloom-brightness-edge-enabled": setBrightnessEdgeEnabled,
    "bloom-theme-mode": setThemeMode,
    "bloom-theme-color": setThemeColor,
    "bloom-theme-opacity": setThemeOpacity,
    "bloom-theme-saturation": setThemeSaturation,
    "bloom-theme-brightness": setThemeBrightness,
    "bloom-weather-city": (v) => setCityName(v || ""),
  });

  // ── Listen for system accent changes (adaptive theme) ──
  useEffect(() => {
    const unlisten = listen<string>("system-accent-changed", (event) => {
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
      unlisten.then((fn) => fn());
    };
  }, []);

  // ── City search debounce ──
  useEffect(() => {
    if (cityName.trim().length < 2) {
      setCitySearchResults([]);
      setShowCityDropdown(false);
      return;
    }

    const timeout = setTimeout(async () => {
      try {
        const res = await fetch(
          `https://geocoding-api.open-meteo.com/v1/search?name=${encodeURIComponent(cityName)}&count=5&language=en&format=json`
        );
        const data = await res.json();
        if (data.results && data.results.length > 0) {
          setCitySearchResults(
            data.results.map((r: any) => ({
              name: r.name,
              country: r.country || "",
              latitude: r.latitude,
              longitude: r.longitude,
            }))
          );
          setShowCityDropdown(true);
        } else {
          setCitySearchResults([]);
        }
      } catch {
        setCitySearchResults([]);
      }
    }, 300);

    return () => clearTimeout(timeout);
  }, [cityName]);

  // ── Update checker ──
  const checkForUpdates = async (manual = true) => {
    setUpdateStatus("checking");
    try {
      const result = await invoke<UpdateCheckResult>("check_for_updates", { force: manual });
      if (result.available) {
        setUpdateStatus("available");
        setUpdateVersion(result.version ?? "");
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
      setUpdateStatus("downloading");
      await invoke("install_update");
      setUpdateStatus("idle");
    } catch (e) {
      console.error(e);
      setUpdateStatus("error");
    }
  };

  // Reflect install progress triggered from any window (including auto-update).
  useEffect(() => {
    const unlisten = listen<{ status: string; progress?: number }>("auto-update-status", (event) => {
      switch (event.payload.status) {
        case "downloading":
          setUpdateStatus("downloading");
          break;
        case "installing":
          setUpdateStatus("installing");
          break;
        case "done":
          setUpdateStatus((prev) => (prev === "downloading" || prev === "installing" ? "idle" : prev));
          break;
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // ── Autostart ──
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
    } catch (err) {}
  };

  // ── Simple boolean toggles ──
  const toggleWeather = () => {
    const next = !weatherEnabled;
    setWeatherEnabled(next);
    saveSetting("bloom-weather-enabled", String(next));
  };

  const toggleCalendar = () => {
    const next = !calendarEnabled;
    setCalendarEnabled(next);
    saveSetting("bloom-calendar-enabled", String(next));
  };

  const toggleMusicMode = () => {
    const next = !musicModeEnabled;
    setMusicModeEnabled(next);
    saveSetting("bloom-music-mode-enabled", String(next));
  };

  const toggleMusicCompactNotch = () => {
    const next = !musicCompactNotch;
    setMusicCompactNotch(next);
    saveSetting("bloom-music-compact-notch", String(next));
  };

  const toggleMediaLayout = (layout: "classic" | "compact") => {
    setMediaLayout(layout);
    saveSetting("bloom-media-layout", layout);
  };

  const toggleVolumeOverlay = () => {
    const next = !volumeOverlayEnabled;
    setVolumeOverlayEnabled(next);
    saveSetting("bloom-volume-overlay-enabled", String(next));
  };

  const toggleVolumeEdge = () => {
    const next = !volumeEdgeEnabled;
    setVolumeEdgeEnabled(next);
    saveSetting("bloom-volume-edge-enabled", String(next));
  };

  const toggleBrightnessOverlay = () => {
    const next = !brightnessOverlayEnabled;
    setBrightnessOverlayEnabled(next);
    saveSetting("bloom-brightness-overlay-enabled", String(next));
  };

  const toggleBrightnessEdge = () => {
    const next = !brightnessEdgeEnabled;
    setBrightnessEdgeEnabled(next);
    saveSetting("bloom-brightness-edge-enabled", String(next));
  };

  const toggleAmbience = () => {
    const next = !mediaAmbienceEnabled;
    setMediaAmbienceEnabled(next);
    saveSetting("bloom-media-ambience-enabled", String(next));
  };

  const toggleCompactGlow = () => {
    const next = !mediaCompactGlowEnabled;
    setMediaCompactGlowEnabled(next);
    saveSetting("bloom-media-compact-glow-enabled", String(next));
  };

  const toggleCorners = () => {
    const next = !cornersEnabled;
    setCornersEnabled(next);
    saveSetting("bloom-corners-enabled", String(next));
  };

  const toggleUpdateIndicator = () => {
    const next = !showUpdateIndicator;
    setShowUpdateIndicator(next);
    saveSetting("bloom-show-update-indicator", String(next));
  };

  const toggleTimeFormat24h = () => {
    const next = !timeFormat24h;
    setTimeFormat24h(next);
    saveSetting("bloom-time-format-24h", String(next));
  };

  const toggleTempUnit = () => {
    const next = !tempUnitFahrenheit;
    setTempUnitFahrenheit(next);
    saveSetting("bloom-temp-unit", next ? "fahrenheit" : "celsius");
  };

  const toggleDock = () => {
    const next = !dockEnabled;
    setDockEnabled(next);
    saveSetting("bloom-dock-enabled", String(next));
  };

  const toggleDockPreview = () => {
    const next = !dockPreviewEnabled;
    setDockPreviewEnabled(next);
    saveSetting("bloom-dock-preview-enabled", String(next));
  };

  const toggleDockIconOnly = () => {
    const next = !dockIconOnly;
    setDockIconOnly(next);
    saveSetting("bloom-dock-icon-only", String(next));
  };

  const toggleDockAdaptive = () => {
    const next = !dockAdaptive;
    setDockAdaptive(next);
    saveSetting("bloom-dock-adaptive", String(next));
  };

  const toggleAutoUpdate = () => {
    const next = !autoUpdate;
    setAutoUpdate(next);
    saveSetting("bloom-auto-update", String(next));
  };

  // ── Value setters ──
  const setDockModeValue = (newMode: string) => {
    setDockMode(newMode);
    saveSetting("bloom-dock-mode", newMode);
  };

  const setNotchModeValue = (newMode: string) => {
    setNotchMode(newMode);
    saveSetting("bloom-notch-mode", newMode);
  };

  const handleThresholdChange = (val: number) => {
    setLowBatteryThreshold(val);
    saveSetting("bloom-low-battery-threshold", val.toString());
  };

  const handleScaleChange = (val: number) => {
    setScale(val);
    saveSetting("bloom-scale", val.toString());
  };

  const handleWidgetsChange = (config: WidgetConfig) => {
    setStatusWidgets(config);
    saveSetting("bloom-status-widgets", JSON.stringify(config));
  };

  // ── Theme handlers ──
  const handleThemeModeChange = async (mode: string) => {
    setThemeMode(mode);
    saveSetting("bloom-theme-mode", mode);

    if (mode === "adaptive") {
      try {
        const accentHex = await invoke<string>("get_system_accent_color");
        const hsl = hexToHsl(accentHex);
        setThemeSaturation(hsl.s / 100);
        saveSetting("bloom-theme-saturation", String(hsl.s / 100));
        setThemeBrightness(hsl.l / 100);
        saveSetting("bloom-theme-brightness", String(hsl.l / 100));
      } catch (e) {
        console.error("Failed to parse adaptive accent HSL:", e);
      }
    }
  };

  const handleThemeColorChange = (color: string) => {
    setThemeColor(color);
    saveSetting("bloom-theme-color", color);

    try {
      const hsl = hexToHsl(color);
      setThemeSaturation(hsl.s / 100);
      saveSetting("bloom-theme-saturation", String(hsl.s / 100));
      setThemeBrightness(hsl.l / 100);
      saveSetting("bloom-theme-brightness", String(hsl.l / 100));
    } catch (e) {
      console.error("Failed to parse custom color HSL:", e);
    }
  };

  const handleOpacityChange = (value: number) => {
    setThemeOpacity(value);
    saveSetting("bloom-theme-opacity", String(value));
  };

  const handleSaturationChange = (value: number) => {
    setThemeSaturation(value);
    saveSetting("bloom-theme-saturation", String(value));
  };

  const handleBrightnessChange = (value: number) => {
    setThemeBrightness(value);
    saveSetting("bloom-theme-brightness", String(value));
  };

  // ── City search ──
  const selectCity = async (city: {
    name: string;
    country: string;
    latitude: number;
    longitude: number;
  }) => {
    setCityName(city.name);
    setShowCityDropdown(false);
    setCitySearchResults([]);
    localStorage.setItem("bloom-weather-city", city.name);
    localStorage.setItem("bloom-weather-lat", city.latitude.toString());
    localStorage.setItem("bloom-weather-lon", city.longitude.toString());
    await invoke("save_setting", { key: "bloom-weather-lat", value: city.latitude.toString() }).catch(() => {});
    await invoke("save_setting", { key: "bloom-weather-lon", value: city.longitude.toString() }).catch(() => {});
    await invoke("save_setting", { key: "bloom-weather-city", value: city.name }).catch(() => {});
    emit("weather-refresh", { lat: city.latitude, lon: city.longitude });
  };

  const handleCityClear = async () => {
    setCityName("");
    setShowCityDropdown(false);
    setCitySearchResults([]);
    localStorage.removeItem("bloom-weather-city");
    localStorage.removeItem("bloom-weather-lat");
    localStorage.removeItem("bloom-weather-lon");
    await invoke("save_setting", { key: "bloom-weather-lat", value: null }).catch(() => {});
    await invoke("save_setting", { key: "bloom-weather-lon", value: null }).catch(() => {});
    await invoke("save_setting", { key: "bloom-weather-city", value: null }).catch(() => {});
    emit("weather-refresh", true);
  };

  // ── Export / Import ──
  const handleExportSettings = async () => {
    setExportStatus("exporting");
    try {
      const { save: saveDialog } = await import("@tauri-apps/plugin-dialog");
      const settingsJson = await invoke<string>("export_settings");
      const filePath = await saveDialog({
        title: "Export Bloom Settings",
        defaultPath: "bloom-settings.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });
      if (filePath) {
        await invoke("write_settings_to_path", { path: filePath, content: settingsJson });
        setExportStatus("success");
        setTimeout(() => setExportStatus("idle"), 2000);
      } else {
        setExportStatus("idle");
      }
    } catch (e) {
      console.error("Export failed:", e);
      setExportStatus("error");
      setTimeout(() => setExportStatus("idle"), 3000);
    }
  };

  const handleImportSettings = async () => {
    setImportStatus("importing");
    try {
      const { open: openDialog } = await import("@tauri-apps/plugin-dialog");
      const filePath = await openDialog({
        title: "Import Bloom Settings",
        filters: [{ name: "JSON", extensions: ["json"] }],
        multiple: false,
      });
      if (filePath) {
        const content = await invoke<string>("read_settings_from_path", { path: filePath as string });
        await invoke("import_settings", { settings: content });
        await loadAllSettings();
        setImportStatus("success");
        setTimeout(() => setImportStatus("idle"), 2000);
      } else {
        setImportStatus("idle");
      }
    } catch (e) {
      console.error("Import failed:", e);
      setImportStatus("error");
      setTimeout(() => setImportStatus("idle"), 3000);
    }
  };

  return {
    // System
    autostart,
    toggleAutostart,
    autoUpdate,
    toggleAutoUpdate,
    lowBatteryThreshold,
    handleThresholdChange,
    timeFormat24h,
    toggleTimeFormat24h,
    showUpdateIndicator,
    toggleUpdateIndicator,
    scale,
    handleScaleChange,
    cornersEnabled,
    toggleCorners,

    // Theme
    themeMode,
    handleThemeModeChange,
    themeColor,
    handleThemeColorChange,
    themeOpacity,
    handleOpacityChange,
    themeSaturation,
    handleSaturationChange,
    themeBrightness,
    handleBrightnessChange,

    // Notch
    notchMode,
    setNotchModeValue,
    calendarEnabled,
    toggleCalendar,
    musicModeEnabled,
    toggleMusicMode,
    musicCompactNotch,
    toggleMusicCompactNotch,
    mediaLayout,
    toggleMediaLayout,
    mediaAmbienceEnabled,
    toggleAmbience,
    mediaCompactGlowEnabled,
    toggleCompactGlow,

    // Weather
    weatherEnabled,
    toggleWeather,
    tempUnitFahrenheit,
    toggleTempUnit,
    cityName,
    setCityName,
    citySearchResults,
    showCityDropdown,
    setShowCityDropdown,
    selectCity,
    handleCityClear,

    // Widgets
    statusWidgets,
    handleWidgetsChange,

    // Dock
    dockEnabled,
    toggleDock,
    dockMode,
    setDockModeValue,
    dockPreviewEnabled,
    toggleDockPreview,
    dockIconOnly,
    toggleDockIconOnly,
    dockAdaptive,
    toggleDockAdaptive,

    // Overlays
    volumeOverlayEnabled,
    toggleVolumeOverlay,
    volumeEdgeEnabled,
    toggleVolumeEdge,
    brightnessOverlayEnabled,
    toggleBrightnessOverlay,
    brightnessEdgeEnabled,
    toggleBrightnessEdge,

    // Updates
    updateStatus,
    updateVersion,
    appVersion,
    checkForUpdates,
    installUpdate,

    // Import / Export
    exportStatus,
    importStatus,
    handleExportSettings,
    handleImportSettings,

    // Utilities
    restartBloom: () => invoke("restart_bloom"),
    quitBloom: () => invoke("quit_bloom"),
  };
}
