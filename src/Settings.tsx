import { StrictMode, useState, useEffect } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow, Effect } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import "./Settings.css";

const appWindow = getCurrentWindow();

function SettingsApp() {
  const [autostart, setAutostart] = useState(false);
  const [weatherEnabled, setWeatherEnabled] = useState(true);
  const [calendarEnabled, setCalendarEnabled] = useState(true);
  const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(true);
  const [mediaVisualizerEnabled, setMediaVisualizerEnabled] = useState(true);
  const [mediaAlbumArtEnabled, setMediaAlbumArtEnabled] = useState(true);
  const [mediaDetailsEnabled, setMediaDetailsEnabled] = useState(true);
  const [cornersEnabled, setCornersEnabled] = useState(() => localStorage.getItem("bloom-corners-enabled") !== "false");
  const [tempUnitFahrenheit, setTempUnitFahrenheit] = useState(false);
  const [cityName, setCityName] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [dockEnabled, setDockEnabled] = useState(false);
  const [dockMode, setDockMode] = useState("fixed");

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
    const weather = localStorage.getItem("bloom-weather-enabled");
    if (weather !== null) setWeatherEnabled(weather === "true");

    const calendar = localStorage.getItem("bloom-calendar-enabled");
    if (calendar !== null) setCalendarEnabled(calendar === "true");

    const volume = localStorage.getItem("bloom-volume-overlay-enabled");
    if (volume !== null) setVolumeOverlayEnabled(volume === "true");

    const visualizer = localStorage.getItem("bloom-media-visualizer-enabled");
    if (visualizer !== null) setMediaVisualizerEnabled(visualizer === "true");

    const art = localStorage.getItem("bloom-media-album-art-enabled");
    if (art !== null) setMediaAlbumArtEnabled(art === "true");

    const details = localStorage.getItem("bloom-media-details-enabled");
    if (details !== null) setMediaDetailsEnabled(details === "true");

    const corners = localStorage.getItem("bloom-corners-enabled");
    if (corners !== null) setCornersEnabled(corners === "true");

    const tempUnit = localStorage.getItem("bloom-temp-unit");
    if (tempUnit !== null) setTempUnitFahrenheit(tempUnit === "fahrenheit");

    const savedCity = localStorage.getItem("bloom-weather-city");
    if (savedCity) setCityName(savedCity);

    const dock = localStorage.getItem("bloom-dock-enabled");
    if (dock !== null) setDockEnabled(dock === "true");

    const dMode = localStorage.getItem("bloom-dock-mode");
    if (dMode) setDockMode(dMode);
  }, []);

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

  const toggleWeather = () => {
    const newVal = !weatherEnabled;
    setWeatherEnabled(newVal);
    localStorage.setItem("bloom-weather-enabled", String(newVal));
    notifyChange("weather", newVal);
  };

  const toggleCalendar = () => {
    const newVal = !calendarEnabled;
    setCalendarEnabled(newVal);
    localStorage.setItem("bloom-calendar-enabled", String(newVal));
    notifyChange("calendar", newVal);
  };

  const toggleVolumeOverlay = () => {
    const newVal = !volumeOverlayEnabled;
    setVolumeOverlayEnabled(newVal);
    localStorage.setItem("bloom-volume-overlay-enabled", String(newVal));
    notifyChange("volume-overlay", newVal);
  };

  const toggleVisualizer = () => {
    const newVal = !mediaVisualizerEnabled;
    setMediaVisualizerEnabled(newVal);
    localStorage.setItem("bloom-media-visualizer-enabled", String(newVal));
    notifyChange("visualizer", newVal);
  };

  const toggleAlbumArt = () => {
    const newVal = !mediaAlbumArtEnabled;
    setMediaAlbumArtEnabled(newVal);
    localStorage.setItem("bloom-media-album-art-enabled", String(newVal));
    notifyChange("album-art", newVal);
  };

  const toggleMediaDetails = () => {
    const newVal = !mediaDetailsEnabled;
    setMediaDetailsEnabled(newVal);
    localStorage.setItem("bloom-media-details-enabled", String(newVal));
    notifyChange("media-details", newVal);
  };

  const toggleCorners = () => {
    const newVal = !cornersEnabled;
    setCornersEnabled(newVal);
    localStorage.setItem("bloom-corners-enabled", String(newVal));
    notifyChange("corners-enabled", newVal);
  };

  const toggleTempUnit = () => {
    const newVal = !tempUnitFahrenheit;
    setTempUnitFahrenheit(newVal);
    localStorage.setItem("bloom-temp-unit", newVal ? "fahrenheit" : "celsius");
    notifyChange("temp-unit", newVal);
  };

  const toggleDock = () => {
    const newVal = !dockEnabled;
    setDockEnabled(newVal);
    localStorage.setItem("bloom-dock-enabled", String(newVal));
    notifyChange("dock-enabled", newVal);
  };

  const toggleDockMode = (newMode: string) => {
    setDockMode(newMode);
    localStorage.setItem("bloom-dock-mode", newMode);
    notifyChange("dock-mode", newMode);
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
        localStorage.setItem("bloom-weather-city", name);
        localStorage.setItem("bloom-weather-lat", latitude.toString());
        localStorage.setItem("bloom-weather-lon", longitude.toString());
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

  return (
    <div className="settings-container">
      <div className="title-bar" data-tauri-drag-region>
        <span className="title-text" data-tauri-drag-region>Settings</span>
        <button className="close-btn" onClick={handleClose} title="Close Settings">
          <svg style={{ pointerEvents: 'none' }} width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
            <line x1="18" y1="6" x2="6" y2="18"></line>
            <line x1="6" y1="6" x2="18" y2="18"></line>
          </svg>
        </button>
      </div>

      <div className="settings-content">
        <div className="setting-group-label">Startup & Display</div>
        <div className="setting-group">
          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#007aff' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M12 2v20M2 12h20" strokeLinecap="round" />
              </svg>
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
            <div className="setting-icon-bg" style={{ background: '#5856d6' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <rect x="3" y="3" width="18" height="18" rx="4" />
              </svg>
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
            <div className="setting-icon-bg" style={{ background: '#34c759' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <rect x="2" y="3" width="20" height="14" rx="2" ry="2" />
                <line x1="8" y1="21" x2="16" y2="21" />
                <line x1="12" y1="17" x2="12" y2="21" />
              </svg>
            </div>
            <div className="setting-info">
              <span className="setting-label">macOS Dock</span>
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
                <div className="setting-info" style={{ marginLeft: '42px' }}>
                  <span className="setting-label">Behavior</span>
                </div>
                <select 
                  className="settings-select" 
                  value={dockMode} 
                  onChange={(e) => toggleDockMode(e.target.value)}
                >
                  <option value="fixed">Fixed (Reserves Space)</option>
                  <option value="auto-hide">Auto Hide</option>
                </select>
              </div>
            </>
          )}
        </div>

        <div className="setting-group-label">Feature Modules</div>
        <div className="setting-group">
          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#32ade6' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M12 2v2M4.93 4.93l1.41 1.41M2 12h2M4.93 19.07l1.41-1.41M12 20v2M17.66 17.66l1.41 1.41M20 12h2M17.66 6.34l1.41-1.41" strokeLinecap="round" />
                <circle cx="12" cy="12" r="4" />
              </svg>
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
          
          <div className="setting-divider" />

          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#ff3b30' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <rect x="3" y="4" width="18" height="18" rx="2" ry="2" />
                <line x1="16" y1="2" x2="16" y2="6" />
                <line x1="8" y1="2" x2="8" y2="6" />
                <line x1="3" y1="10" x2="21" y2="10" />
              </svg>
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
            <div className="setting-icon-bg" style={{ background: '#ff9500' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M11 5L6 9H2v6h4l5 4V5zM15.54 8.46a5 5 0 0 1 0 7.07" />
              </svg>
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
        </div>

        <div className="setting-group-label">Media Aesthetic</div>
        <div className="setting-group">
          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#af52de' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M12 20V10M18 20V4M6 20v-4" />
              </svg>
            </div>
            <div className="setting-info">
              <span className="setting-label">Visualizer bars</span>
              <span className="setting-desc">Audio-reactive animation</span>
            </div>
            <label className="toggle-switch">
              <input type="checkbox" checked={mediaVisualizerEnabled} onChange={toggleVisualizer} />
              <span className="slider"></span>
            </label>
          </div>
          
          <div className="setting-divider" />

          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#ff2d55' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M9 18V5l12-2v13M9 18c0 1.1-1.34 2-3 2s-3-.9-3-2 1.34-2 3-2 3 .9 3 2zm12-2c0 1.1-1.34 2-3 2s-3-.9-3-2 1.34-2 3-2 3 .9 3 2z" />
              </svg>
            </div>
            <div className="setting-info">
              <span className="setting-label">Album Artwork</span>
              <span className="setting-desc">Show high-res covers</span>
            </div>
            <label className="toggle-switch">
              <input type="checkbox" checked={mediaAlbumArtEnabled} onChange={toggleAlbumArt} />
              <span className="slider"></span>
            </label>
          </div>

          <div className="setting-divider" />

          <div className="setting-item">
            <div className="setting-icon-bg" style={{ background: '#5ac8fa' }}>
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="white" strokeWidth="2.5">
                <path d="M4 6h16M4 12h16M4 18h7" strokeLinecap="round" />
              </svg>
            </div>
            <div className="setting-info">
              <span className="setting-label">Song Details</span>
              <span className="setting-desc">Hover marquee info</span>
            </div>
            <label className="toggle-switch">
              <input type="checkbox" checked={mediaDetailsEnabled} onChange={toggleMediaDetails} />
              <span className="slider"></span>
            </label>
          </div>
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
