import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback, useRef, memo } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./App.css";

// Simple SVG icons
function WifiIcon({ connected }: { connected: boolean }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" opacity={connected ? 1 : 0.4}>
      <path d="M5 12.55a11 11 0 0 1 14.08 0" />
      <path d="M1.42 9a16 16 0 0 1 21.16 0" />
      <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
      <line x1="12" y1="20" x2="12.01" y2="20" />
    </svg>
  );
}

function BellOffIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
      <path d="M18.63 13A17.89 17.89 0 0 1 18 8" />
      <path d="M6.26 6.26A5.86 5.86 0 0 0 6 8c0 7-3 9-3 9h14" />
      <path d="M18 8a6 6 0 0 0-9.33-5" />
      <line x1="1" y1="1" x2="23" y2="23" />
    </svg>
  );
}

function BellIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
  );
}

function ThermometerIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M14 4v10.54a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0Z" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="3"></circle>
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z"></path>
    </svg>
  );
}

function BatteryIcon({ charging, level, threshold = 20 }: { charging: boolean; level: number; threshold?: number }) {
  const percentage = Math.min(Math.max(level, 0), 100);
  
  return (
    <div style={{ 
      display: 'flex', 
      alignItems: 'center', 
      position: 'relative',
      height: '14px',
      justifyContent: 'center'
    }}>
      <svg width="20" height="10" viewBox="0 0 20 10" fill="none">
        {/* Battery Shell - Centered at 9px within 20px width, ignoring the tip's offset */}
        <rect 
          x="2" y="0.75" width="14" height="8.5" rx="2.4" 
          stroke="currentColor" strokeOpacity={0.35} strokeWidth="1.1"
        />
        {/* Battery Tip */}
        <path 
           d="M17.5 3.5V6.5" 
           stroke="currentColor" strokeOpacity={0.35} strokeWidth="1.2" strokeLinecap="round" 
        />
        {/* Fill */}
        <rect 
          x="3.8" y="2.5" 
          width={Math.max(0.5, (percentage / 100) * 10.4)} 
          height="5" rx="1" 
          fill={charging ? "#32D74B" : (percentage <= threshold ? "#FF453A" : "white")} 
        />
      </svg>
      {/* Charging Bolt - Centered on the battery body */}
      {charging && (
        <div style={{ 
          position: 'absolute', 
          top: '50%', 
          left: '9px', 
          transform: 'translate(-50%, -50%)',
          color: 'white',
          filter: 'drop-shadow(0px 0px 1.5px rgba(0,0,0,0.8))'
        }}>
          <svg width="7" height="10" viewBox="0 0 8 12" fill="currentColor">
            <path d="M4.5 0L0 7H3.5L2.5 12L8 5H4.5L5.5 0H4.5Z" />
          </svg>
        </div>
      )}
    </div>
  );
}

export const Visualizer = memo(function Visualizer({ isPlaying, bars = 5, height = 20 }: { isPlaying: boolean; bars?: number; height?: number }) {
  const [audioData, setAudioData] = useState<number[]>(new Array(bars).fill(0.18));

  useEffect(() => {
    if (!isPlaying) {
      setAudioData(new Array(bars).fill(0.18));
      return;
    }

    const unlisten = listen<{ frequencies: number[] }>("audio-visualization", (event) => {
      // If we receive fewer frequencies than bars, repeat or interpolate
      // If more, slice
      let data = event.payload.frequencies;
      if (data.length > bars) data = data.slice(0, bars);
      while (data.length < bars) data.push(0.18);
      setAudioData(data);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, [isPlaying, bars]);

  return (
    <div className="visualizer-horizontal" style={{ height: `${height}px`, width: `${bars * 6}px` }}>
      {audioData.map((value, i) => (
        <motion.div
          key={i}
          className="bar-horizontal"
          animate={{
            scaleY: isPlaying ? Math.max(0.2, value) : 0.1,
            opacity: isPlaying ? 0.95 : 0.5
          }}
          transition={{
            type: "spring",
            stiffness: 600,
            damping: 30,
            mass: 0.5
          }}
        />
      ))}
    </div>
  );
});

interface MediaInfo {
  title: string;
  artist: string;
  is_playing: boolean;
  has_media: boolean;
  artwork?: string[];
}

function MarqueeText({ title, artist }: { title: string, artist: string }) {
  const containerRef = useRef<HTMLDivElement>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const [shouldMarquee, setShouldMarquee] = useState(false);

  useEffect(() => {
    if (containerRef.current && contentRef.current) {
      setShouldMarquee(contentRef.current.scrollWidth > containerRef.current.clientWidth + 2);
    }
  }, [title, artist]);

  return (
    <div className="media-details" ref={containerRef}>
      <div
        className={`marquee-content ${shouldMarquee ? 'should-animate' : ''}`}
        ref={contentRef}
      >
        <span className="title">{title}</span>
        <span className="dot">•</span>
        <span className="artist">{artist}</span>
      </div>
    </div>
  );
}

function App() {
  const [time, setTime] = useState("");
  const [isHovered, setIsHovered] = useState(false);
  const [isReady, setIsReady] = useState(false);

  // Battery state
  const [batteryLevel, setBatteryLevel] = useState(100);
  const [isCharging, setIsCharging] = useState(false);
  const [showPowerPulse, setShowPowerPulse] = useState(false);
  const [showLowBatteryPulse, setShowLowBatteryPulse] = useState(false);
  const [lowBatteryThreshold, setLowBatteryThreshold] = useState(() => parseInt(localStorage.getItem("bloom-low-battery-threshold") || "20"));
  const prevChargingRef = useRef<boolean | null>(null);
  const powerPulseTimeoutRef = useRef<any>(null);
  const lowBatteryPulseShownRef = useRef<boolean>(false);

  useEffect(() => {
    if (isReady && prevChargingRef.current !== null && prevChargingRef.current !== isCharging) {
      setShowPowerPulse(true);
      if (powerPulseTimeoutRef.current) clearTimeout(powerPulseTimeoutRef.current);
      powerPulseTimeoutRef.current = setTimeout(() => {
        setShowPowerPulse(false);
      }, 4000);
    }
    prevChargingRef.current = isCharging;
  }, [isCharging, isReady]);

  useEffect(() => {
    // Trigger pulse when dropping below threshold while discharging
    if (isReady && batteryLevel <= lowBatteryThreshold && !isCharging && !lowBatteryPulseShownRef.current) {
      setShowLowBatteryPulse(true);
      lowBatteryPulseShownRef.current = true;
      setTimeout(() => setShowLowBatteryPulse(false), 5000);
    }
    
    // Reset the "shown" state if battery is charged or threshold is lowered
    if (isCharging || batteryLevel > lowBatteryThreshold) {
      lowBatteryPulseShownRef.current = false;
    }
  }, [batteryLevel, isCharging, lowBatteryThreshold, isReady]);

  // Weather state
  const [temperature, setTemperature] = useState<number | null>(null);
  const [weatherCondition, setWeatherCondition] = useState<string>("");

  // Network state
  const [isOnline, setIsOnline] = useState(navigator.onLine);

  // Media state
  const [isPlaying, setIsPlaying] = useState(false);
  const [mediaInfo, setMediaInfo] = useState<MediaInfo>({
    title: "",
    artist: "",
    is_playing: false,
    has_media: false
  });
  const [albumArtUrl, setAlbumArtUrl] = useState<string | null>(null);
  const [windowLabel] = useState<string>(getCurrentWebviewWindow().label);
  const [isVisible, setIsVisible] = useState(true);
  const [isImpacted, setIsImpacted] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);

  useEffect(() => {
    // Only animate the main top-bar
    if (windowLabel !== 'main') {
      setIsReady(true);
      setIsImpacted(true);
      setIsExpanded(true);
      return;
    }

    const checkVisibility = async () => {
      try {
        const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
        const win = getCurrentWebviewWindow();
        const visible = await win.isVisible();
        if (visible) {
          setIsReady(true);
          // Impact and Expansion start together once it reaches the top
          setTimeout(() => {
            setIsImpacted(true);
            setIsExpanded(true);
          }, 240);
          return true;
        }
      } catch (e) {}
      return false;
    };

    const interval = setInterval(async () => {
      if (await checkVisibility()) clearInterval(interval);
    }, 100);
    
    checkVisibility();
    return () => clearInterval(interval);
  }, [windowLabel]);

  // Settings state
  const [settingsWeatherEnabled, setSettingsWeatherEnabled] = useState(() => localStorage.getItem("bloom-weather-enabled") !== "false");
  const [settingsCalendarEnabled, setSettingsCalendarEnabled] = useState(() => localStorage.getItem("bloom-calendar-enabled") !== "false");
  const [settingsVisualizerEnabled, setSettingsVisualizerEnabled] = useState(() => localStorage.getItem("bloom-visualizer-enabled") !== "false");
  const [settingsAlbumArtEnabled, setSettingsAlbumArtEnabled] = useState(() => localStorage.getItem("bloom-media-album-art-enabled") !== "false");
  const [settingsMediaDetailsEnabled, setSettingsMediaDetailsEnabled] = useState(() => localStorage.getItem("bloom-media-details-enabled") !== "false");
  const [settingsCornersEnabled, setSettingsCornersEnabled] = useState(() => localStorage.getItem("bloom-corners-enabled") !== "false");
  const [tempUnit, setTempUnit] = useState(() => localStorage.getItem("bloom-temp-unit") || "celsius");

  useEffect(() => {
    // On startup, we don't need to call toggle_corners_window anymore as it's built-in
    if (windowLabel === 'main') {

      // Add a small delay to ensure windows are created and ready
      setTimeout(() => {
        const dockEnabled = localStorage.getItem("bloom-dock-enabled") === "true";
        if (dockEnabled) {
          invoke("toggle_dock", { enable: true });
          invoke("change_dock_mode", { mode: localStorage.getItem("bloom-dock-mode") || "fixed" });
        }
        // Re-sync topbar to prevent displacement (Always do this on launch)
        invoke("sync_appbar");
        // Secondary sync to catch any shell-level work-area jumps
        setTimeout(() => invoke("sync_appbar"), 1000);
      }, 1000);
    }
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    // Disable context menu globally
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext);

    const unlistenVisibility = listen<boolean>("visibility-change", (event) => {
      setIsVisible(event.payload);
    });
    const unlistenSettings = listen<{ key: string, value: any }>("settings-changed", (event) => {
      const { key, value } = event.payload;
      if (key === "weather") setSettingsWeatherEnabled(value);
      if (key === "calendar") setSettingsCalendarEnabled(value);
      if (key === "visualizer") setSettingsVisualizerEnabled(value);
      if (key === "album-art") setSettingsAlbumArtEnabled(value);
      if (key === "media-details") setSettingsMediaDetailsEnabled(value);
      if (key === "temp-unit") setTempUnit(value ? "fahrenheit" : "celsius");
      if (key === "weather-refresh") {
        // Re-trigger the init function or just update from localStorage
        window.dispatchEvent(new CustomEvent("refresh-weather"));
      }
      if (key === "corners-enabled") {
        setSettingsCornersEnabled(value as boolean);
      }
      if (key === "low-battery-threshold") {
        setLowBatteryThreshold(value);
      }
      if (key === "dock-enabled") {
        if (windowLabel === 'main') {
          invoke("toggle_dock", { enable: value });
          if (value) {
            invoke("change_dock_mode", { mode: localStorage.getItem("bloom-dock-mode") || "fixed" });
          }
          // Always re-sync topbar to prevent displacement when dock state changes
          setTimeout(() => invoke("sync_appbar"), 200);
        }
      }
      if (key === "dock-mode") {
        if (windowLabel === 'main') {
          invoke("change_dock_mode", { mode: value });
          setTimeout(() => invoke("sync_appbar"), 200);
        }
      }
    });

    return () => {
      unlistenVisibility.then(f => f());
      unlistenSettings.then(f => f());
    };
  }, []);

  // New bloom mode state: 'status', 'music', or 'calendar'
  const [bloomMode, setBloomMode] = useState<'status' | 'music' | 'calendar'>('status');
  const [isMuted] = useState(false);

  // Reset window height when state changes
  useEffect(() => {
    if (!isExpanded) return;
    let timeout: any;
    if (bloomMode === 'calendar') {
      invoke("set_window_height", { height: 300 });
    } else if (isHovered) {
      // Keep window tall if media is present to allow smooth toggling without clipping
      const h = (mediaInfo.has_media) ? 130 : 64;
      invoke("set_window_height", { height: h });
    } else {
      // Shorter timeout to reduce "glitchy" dead-zone feel
      timeout = setTimeout(() => {
        invoke("set_window_height", { height: 48 });
      }, 400);
    }
    return () => clearTimeout(timeout);
  }, [isHovered, bloomMode === 'calendar', isExpanded, mediaInfo.has_media]);

  // Timer state
  const [timerSeconds, setTimerSeconds] = useState(0);
  const [isTimerRunning, setIsTimerRunning] = useState(false);
  const [isCompactTimerVisible, setIsCompactTimerVisible] = useState(false);
  const [isTimerFinished, setIsTimerFinished] = useState(false);
  const timerIntervalRef = useRef<any>(null);

  const formatTimerTime = (totalSeconds: number) => {
    const mins = Math.floor(Math.abs(totalSeconds) / 60);
    const secs = Math.abs(totalSeconds) % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  const startTimer = (mins: number) => {
    setTimerSeconds(mins * 60);
    setIsTimerRunning(true);
    setIsTimerFinished(false);
  };

  const toggleTimer = () => setIsTimerRunning(!isTimerRunning);
  const resetTimer = () => {
    setIsTimerRunning(false);
    setTimerSeconds(0);
    setIsTimerFinished(false);
  };

  useEffect(() => {
    if (isTimerRunning && timerSeconds > 0) {
      timerIntervalRef.current = setInterval(() => {
        setTimerSeconds(s => s - 1);
      }, 1000);
    } else if (isTimerRunning && timerSeconds === 0) {
      setIsTimerRunning(false);
      setIsTimerFinished(true);
    }
    return () => {
      if (timerIntervalRef.current) clearInterval(timerIntervalRef.current);
    };
  }, [isTimerRunning, timerSeconds === 0]);

  const lastTrackRef = useRef<string | null>(null);
  const lastPlayingRef = useRef<boolean>(false);

  // Auto-switch to music mode only when a *new* track starts while playing,
  // or when playback transitions from paused to playing.
  useEffect(() => {
    const isNewTrackWhilePlaying = mediaInfo.title !== lastTrackRef.current && isPlaying;
    const justStartedPlaying = isPlaying && !lastPlayingRef.current;

    // Only auto-switch if we are actually playing something new
    if (mediaInfo.has_media && isPlaying && bloomMode !== 'calendar' && (isNewTrackWhilePlaying || justStartedPlaying)) {
      setBloomMode('music');
    }

    lastTrackRef.current = mediaInfo.title;
    lastPlayingRef.current = isPlaying;
  }, [mediaInfo.has_media, isPlaying, mediaInfo.title]);

  // Auto-switch back to status mode if music stops for 4 seconds
  useEffect(() => {
    let timer: any;
    if (!isPlaying && bloomMode === 'music') {
      timer = setTimeout(() => {
        setBloomMode('status');
      }, 4000);
    }
    return () => clearTimeout(timer);
  }, [isPlaying, bloomMode]);

  // Update time
  useEffect(() => {
    const updateTime = () => {
      const now = new Date();
      setTime(
        now.toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
        })
      );
    };

    updateTime();
    const interval = setInterval(updateTime, 1000);

    // Toggle compact timer view every 5 seconds if running
    let timerToggleInterval: any;
    if (isTimerRunning && bloomMode !== 'calendar') {
      timerToggleInterval = setInterval(() => {
        setIsCompactTimerVisible(prev => !prev);
      }, 5000);
    } else {
      setIsCompactTimerVisible(false);
    }

    return () => {
      clearInterval(interval);
      if (timerToggleInterval) clearInterval(timerToggleInterval);
    };
  }, [isTimerRunning, bloomMode]);

  // Battery API
  useEffect(() => {
    let battery: any = null;

    const initBattery = async () => {
      try {
        battery = await (navigator as any).getBattery();

        const updateBattery = () => {
          setBatteryLevel(Math.round(battery.level * 100));
          setIsCharging(battery.charging);
        };

        updateBattery();

        battery.addEventListener("levelchange", updateBattery);
        battery.addEventListener("chargingchange", updateBattery);

        return () => {
          battery.removeEventListener("levelchange", updateBattery);
          battery.removeEventListener("chargingchange", updateBattery);
        };
      } catch (e) {
        // Battery API not supported
      }
    };

    initBattery();
  }, []);

  // Network status
  useEffect(() => {
    const handleOnline = () => setIsOnline(true);
    const handleOffline = () => setIsOnline(false);

    window.addEventListener("online", handleOnline);
    window.addEventListener("offline", handleOffline);

    return () => {
      window.removeEventListener("online", handleOnline);
      window.removeEventListener("offline", handleOffline);
    };
  }, []);

  // Weather API (Open-Meteo - free, no API key needed)
  useEffect(() => {
    const fetchWeather = async (latitude: number, longitude: number) => {
      try {
        const unitParam = tempUnit === "fahrenheit" ? "&temperature_unit=fahrenheit" : "";
        const response = await fetch(
          `https://api.open-meteo.com/v1/forecast?latitude=${latitude}&longitude=${longitude}&current=temperature_2m,weather_code,is_day&timezone=auto${unitParam}`
        );
        const data = await response.json();
        const temp = data.current.temperature_2m;
        setTemperature(Math.round(temp));

        // Simple weather code mapping
        const code = data.current.weather_code;
        const conditions: Record<number, string> = {
          0: "Clear",
          1: "Mostly Clear",
          2: "Partly Cloudy",
          3: "Overcast",
          45: "Foggy",
          48: "Foggy",
          51: "Drizzle",
          53: "Drizzle",
          55: "Drizzle",
          61: "Rainy",
          63: "Rainy",
          65: "Rainy",
          71: "Snowy",
          73: "Snowy",
          75: "Snowy",
          95: "Stormy",
          96: "Stormy",
          99: "Stormy",
          224: "Stormy",
        };
        setWeatherCondition(conditions[code] || "Unknown");
      } catch (e) {
        console.log("Weather fetch failed");
      }
    };

    const init = async () => {
      try {
        // Check for manual coordinates first
        const savedLat = localStorage.getItem("bloom-weather-lat");
        const savedLon = localStorage.getItem("bloom-weather-lon");
        if (savedLat && savedLon) {
          await fetchWeather(parseFloat(savedLat), parseFloat(savedLon));
          return;
        }

        // Fetch location directly via JS instead of using a hidden rust process
        const response = await fetch('https://ipapi.co/json/');
        if (!response.ok) throw new Error("Primary location source failed");

        const data = await response.json();
        const lat = data.latitude || data.lat;
        const lon = data.longitude || data.lon;

        if (lat && lon) {
          await fetchWeather(lat, lon);
        } else {
          // Fallback to second source if fields are missing
          const fallbackRes = await fetch('http://ip-api.com/json/?fields=status,lat,lon,city,country');
          const fallbackData = await fallbackRes.json();
          if (fallbackData.lat && fallbackData.lon) {
            await fetchWeather(fallbackData.lat, fallbackData.lon);
          } else {
            throw new Error("All location sources failed");
          }
        }
      } catch (e) {
        // Fall back to Delhi (safe bet for UTC+5:30)
        await fetchWeather(28.6139, 77.2090);
      }
    };

    init();

    // Listen for manual refreshes from settings
    const handleRefresh = () => init();
    window.addEventListener("refresh-weather", handleRefresh);

    // Refresh weather every 30 minutes
    const interval = setInterval(init, 30 * 60 * 1000);
    return () => {
      clearInterval(interval);
      window.removeEventListener("refresh-weather", handleRefresh);
    };
  }, [tempUnit]);

  // Native Windows Media Controls - Listen for updates from background worker
  useEffect(() => {
    const unlisten = listen<MediaInfo>("media-update", (event) => {
      const info = event.payload;
      if (!info) return;

      setMediaInfo(prev => {
        // Find if artwork changed by checking the first element
        const prevArt = prev.artwork?.[0];
        const nextArt = info.artwork?.[0];
        const artChanged = prevArt !== nextArt;

        if (prev.title === info.title &&
          prev.artist === info.artist &&
          prev.is_playing === info.is_playing &&
          prev.has_media === info.has_media &&
          !artChanged) {
          return prev;
        }

        // Update playing state separately for the hook triggers
        setIsPlaying(info.is_playing);

        if (info.artwork && info.artwork.length > 0) {
          const newArt = info.artwork[0];
          setAlbumArtUrl(prev => prev === newArt ? prev : newArt);
        } else {
          setAlbumArtUrl(null);
        }

        return info;
      });
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  // Media controls via Tauri commands
  const togglePlayPause = useCallback(async () => {
    try {
      await invoke("media_play_pause");
      setIsPlaying(!isPlaying);
    } catch (e) {
      console.error("Failed to toggle play/pause:", e);
    }
  }, [isPlaying]);


  const skipNext = useCallback(async () => {
    try {
      await invoke("media_next");
    } catch (e) {
      console.error("Failed to skip next:", e);
    }
  }, []);

  const skipPrevious = useCallback(async () => {
    try {
      await invoke("media_previous");
    } catch (e) {
      console.error("Failed to skip previous:", e);
    }
  }, []);


  // Open WiFi settings
  const openWifiSettings = useCallback(async () => {
    try {
      await invoke("open_wifi_settings");
    } catch (e) {
      console.error("Failed to open WiFi settings:", e);
    }
  }, []);

  // Open notification center
  const openNotificationCenter = useCallback(async () => {
    try {
      await invoke("open_notification_center");
    } catch (e) {
      console.error("Failed to open notification center:", e);
    }
  }, []);

  const openSettingsWindow = useCallback(async () => {
    try {
      const { WebviewWindow } = await import('@tauri-apps/api/webviewWindow');
      let settingsWebview = await WebviewWindow.getByLabel('settings');

      if (settingsWebview) {
        await settingsWebview.show();
        await settingsWebview.unminimize();
        await settingsWebview.setFocus();
      } else {
        settingsWebview = new WebviewWindow('settings', {
          url: 'settings.html',
          title: 'Settings',
          width: 380,
          height: 450,
          decorations: false,
          transparent: true,
          resizable: false,
          center: true,
          skipTaskbar: false,
        });
        // Windows created via JS need to wait for ready or just show
        await settingsWebview.show();
      }
    } catch (e) {
      console.error("Failed to open settings window:", e);
    }
  }, []);

  const handleBloomClick = () => {
    if (mediaInfo.has_media) {
      setBloomMode(prev => prev === 'music' ? 'status' : 'music');
    }
  };

  const toggleCalendarMode = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isTimerFinished) {
      resetTimer();
      return;
    }
    if (!settingsCalendarEnabled) return;

    setBloomMode(prev => {
      if (prev === 'calendar') {
        // Return to music mode if media is present and playing, otherwise status
        return (mediaInfo.has_media && isPlaying) ? 'music' : 'status';
      }
      return 'calendar';
    });
  };

  // Music mode shows any time we have media info (playing or paused)
  const isMusicMode = mediaInfo.has_media && bloomMode === 'music';

  // Calculate width dynamically based on enabled features
  const getDynamicWidth = () => {
    if (isCalendarMode) return 480;
    if (isMusicMode && isHovered) return 380;
    if ((showPowerPulse || showLowBatteryPulse) && !isHovered) return 200;

    let w = 140;
    if (isMusicMode) {
      w = 140;
      if (settingsVisualizerEnabled && isPlaying) w += 30;
      if (settingsAlbumArtEnabled) w += 30;

      if (isHovered) {
        w += 60;
      }
    } else if (isHovered) {
      w = 320;
    }

    return w;
  };

  const isCalendarMode = bloomMode === 'calendar';



  return (
    <div className="screen" style={{ overflow: 'hidden' }}>
      {/* Screen Corners (Top) */}
      <AnimatePresence>
        {isVisible && settingsCornersEnabled && (
          <>
            <motion.div
              className="screen-corner top-left"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1, filter: "blur(0px)" }}
              exit={{ opacity: 0, filter: "blur(10px)" }}
            />
            <motion.div
              className="screen-corner top-right"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1, filter: "blur(0px)" }}
              exit={{ opacity: 0, filter: "blur(10px)" }}
            />
          </>
        )}
      </AnimatePresence>

      <motion.div
        className={`bloom ${isHovered ? 'expanded' : ''} ${isImpacted ? 'is-impacted' : ''}`}
        initial={{ y: 250, width: 34, height: 34, borderTopLeftRadius: 18, borderTopRightRadius: 18, borderBottomLeftRadius: 18, borderBottomRightRadius: 18, scaleX: 0.9, scaleY: 1.3, opacity: 0 }}
        animate={{
          width: isExpanded ? (isHovered && isMusicMode ? 380 : getDynamicWidth()) : 34,
          height: isExpanded
            ? (isCalendarMode ? 275 : (isHovered && isMusicMode ? 100 : 36))
            : 34,
          y: !isReady ? 250 : (isVisible ? 0 : -80),
          opacity: isVisible ? 1 : 0,
          scaleX: !isReady ? 1 : (isExpanded ? 1 : (isImpacted ? 1.15 : 0.9)),
          scaleY: !isReady ? 1 : (isExpanded ? 1 : (isImpacted ? 0.85 : 1.3)),
          borderTopLeftRadius: isImpacted ? 0 : 18,
          borderTopRightRadius: isImpacted ? 0 : 18,
          borderBottomLeftRadius: 18,
          borderBottomRightRadius: 18,
          filter: isVisible ? "blur(0px)" : "blur(8px)"
        }}
        onClick={() => {
          // Only toggle if not in calendar mode
          if (!isCalendarMode) handleBloomClick();
        }}
        onHoverStart={() => setIsHovered(true)}
        onHoverEnd={() => {
          setIsHovered(false);
          if (bloomMode === 'calendar') {
            setBloomMode(mediaInfo.has_media && isPlaying ? 'music' : 'status');
          }
        }}
        style={{ originY: 0 }}
        transition={{
          width: { type: "spring", stiffness: 400, damping: 28 },
          height: { type: "spring", stiffness: 450, damping: 26 },
          y: { type: "spring", stiffness: 400, damping: 30 },
          opacity: { duration: 0.2 },
          scaleX: { type: "spring", stiffness: 600, damping: 18 },
          scaleY: { type: "spring", stiffness: 600, damping: 18 },
          borderTopLeftRadius: { type: "spring", stiffness: 1000, damping: 40 },
          borderTopRightRadius: { type: "spring", stiffness: 1000, damping: 40 },
          default: { type: "spring", stiffness: 500, damping: 30, mass: 1 }
        }}
      >
        <AnimatePresence mode="wait">
          {isExpanded && (
            <motion.div
              key="bloom-content"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              transition={{ duration: 0.2 }}
              style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center' }}
            >
              {/* Faster Waiting Transition Area */}
              <AnimatePresence mode="wait">
                {isHovered && isMusicMode && !isCalendarMode ? (
                  <motion.div
                    key="expanded-music"
                    className="expanded-music-container"
                    initial={{ opacity: 0, scale: 0.98 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.98, transition: { duration: 0.1 } }}
                    transition={{ type: "spring", stiffness: 500, damping: 30 }}
                  >
                    <div className="compact-premium-layout">
                      <div className="album-art-section">
                        <motion.div
                          className="premium-album-art"
                          whileHover={{ scale: 1.05 }}
                          whileTap={{ scale: 0.95 }}
                        >
                          {albumArtUrl ? (
                            <img src={albumArtUrl} alt="Art" />
                          ) : (
                            <div className="art-placeholder-mini">🎵</div>
                          )}
                        </motion.div>
                      </div>

                      <div className="metadata-controls-section-middle">
                        <div className="track-info-middle">
                          <span className="premium-title">{mediaInfo.title}</span>
                          <span className="premium-artist">{mediaInfo.artist}</span>
                        </div>

                        <div className="controls-row-sleek">
                          <button className="sleek-btn" onClick={(e) => { e.stopPropagation(); skipPrevious(); }}>
                            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                              <path d="M6 6h2v12H6zm3.5 6l8.5 6V6z" />
                            </svg>
                          </button>
                          
                          <button className="sleek-btn play-pause-btn" onClick={(e) => { e.stopPropagation(); togglePlayPause(); }}>
                            {isPlaying ? (
                              <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                                <rect x="6" y="4" width="4" height="16" rx="1" />
                                <rect x="14" y="4" width="4" height="16" rx="1" />
                              </svg>
                            ) : (
                              <svg width="20" height="20" viewBox="0 0 24 24" fill="currentColor">
                                <path d="M8 5v14l11-7z" />
                              </svg>
                            )}
                          </button>

                          <button className="sleek-btn" onClick={(e) => { e.stopPropagation(); skipNext(); }}>
                            <svg width="18" height="18" viewBox="0 0 24 24" fill="currentColor">
                              <path d="M6 18l8.5-6L6 6zM16 6h2v12h-2z" />
                            </svg>
                          </button>
                        </div>
                      </div>

                      <div className="visualizer-section-right">
                        <Visualizer isPlaying={isPlaying} bars={5} height={22} />
                      </div>
                    </div>
                  </motion.div>
                ) : (
                  <motion.div
                    key="standard-view-group"
                    initial={{ opacity: 0, y: -5 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 5, transition: { duration: 0.1 } }}
                    transition={{ duration: 0.2 }}
                    style={{ width: '100%' }}
                  >
                    <div className="main-row">
                      <AnimatePresence mode="wait">
                        {(showPowerPulse || showLowBatteryPulse) && !isHovered ? (
                          <motion.div
                            key="pulse-view"
                            initial={{ opacity: 0, scale: 0.95, filter: "blur(4px)" }}
                            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
                            exit={{ opacity: 0, scale: 1.05, filter: "blur(4px)" }}
                            className="power-pulse-content"
                          >
                            <BatteryIcon charging={isCharging} level={batteryLevel} threshold={lowBatteryThreshold} />
                            <span className="label" style={{ color: showLowBatteryPulse ? "#FF453A" : "inherit" }}>
                              {showLowBatteryPulse ? "Low Battery" : (isCharging ? "Charging" : "On Battery")} • {batteryLevel}%
                            </span>
                          </motion.div>
                        ) : (
                          <motion.div
                            key="standard-view"
                            className="main-row-inner"
                            initial={{ opacity: 0 }}
                            animate={{ opacity: 1 }}
                            exit={{ opacity: 0 }}
                          >
                            {/* Left: visualizer (music) or wifi+notifs (status) */}
                            {(isMusicMode && settingsVisualizerEnabled) || (!isMusicMode && isHovered) ? (
                              <div className="side-content left">
                                <AnimatePresence mode="wait">
                                  {isMusicMode ? (
                                    <AnimatePresence>
                                      {settingsVisualizerEnabled && (
                                        <motion.div
                                          key="visualizer"
                                          initial={{ scale: 0.8, opacity: 0 }}
                                          animate={{ scale: 1, opacity: 1 }}
                                          exit={{ scale: 0.8, opacity: 0 }}
                                        >
                                          <Visualizer isPlaying={isPlaying} />
                                        </motion.div>
                                      )}
                                    </AnimatePresence>
                                  ) : (
                                    isHovered && (
                                      <motion.div
                                        key="left-passive"
                                        className="passive-features-group"
                                        initial={{ opacity: 0, x: -10 }}
                                        animate={{ opacity: 1, x: 0 }}
                                        exit={{ opacity: 0, x: -10 }}
                                      >
                                        <div className="passive-feature clickable" onClick={openWifiSettings}>
                                          <WifiIcon connected={isOnline} />
                                        </div>
                                        <div className="passive-feature clickable" onClick={openNotificationCenter}>
                                          {isMuted ? <BellOffIcon /> : <BellIcon />}
                                        </div>
                                        <div className="passive-feature clickable" onClick={openSettingsWindow}>
                                          <SettingsIcon />
                                        </div>
                                      </motion.div>
                                    )
                                  )}
                                </AnimatePresence>
                              </div>
                            ) : (
                              /* Spacer to keep time centered if the OTHER side has content */
                              isMusicMode && settingsAlbumArtEnabled && <div className="side-content left" />
                            )}

                            {/* Center - Time (always visible) */}
                            <div className="time-flip-container" onClick={toggleCalendarMode}>
                              <AnimatePresence initial={false}>
                                {isCompactTimerVisible || isTimerFinished ? (
                                  <motion.span
                                    key="timer"
                                    className={`time compact-timer ${isTimerFinished ? 'timer-finished' : ''}`}
                                    initial={{ rotateX: -90, opacity: 0 }}
                                    animate={{ rotateX: 0, opacity: 1 }}
                                    exit={{ rotateX: 90, opacity: 0 }}
                                    transition={{ type: "spring", stiffness: 600, damping: 30 }}
                                  >
                                    {formatTimerTime(timerSeconds)}
                                  </motion.span>
                                ) : (
                                  <motion.span
                                    key="clock"
                                    className="time"
                                    initial={{ rotateX: -90, opacity: 0 }}
                                    animate={{ rotateX: 0, opacity: 1 }}
                                    exit={{ rotateX: 90, opacity: 0 }}
                                    transition={{ type: "spring", stiffness: 600, damping: 30 }}
                                  >
                                    {time}
                                  </motion.span>
                                )}
                              </AnimatePresence>
                            </div>

                            {/* Right: album art (music) or battery+temp (status) */}
                            {(isMusicMode && settingsAlbumArtEnabled) || (!isMusicMode && isHovered) ? (
                              <div className="side-content right">
                                <AnimatePresence mode="wait">
                                  {isMusicMode && settingsAlbumArtEnabled ? (
                                    <motion.button
                                      key="album-art"
                                      className={`album-art${isHovered ? ' album-art-large' : ''}${!isPlaying ? ' paused' : ''}`}
                                      initial={{ opacity: 0, scale: 0.8 }}
                                      animate={{ opacity: 1, scale: 1 }}
                                      exit={{ opacity: 0, y: -20, scale: 0.8, filter: "blur(8px)" }}
                                      transition={{ duration: 0.12 }}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        togglePlayPause();
                                      }}
                                      onDoubleClick={(e) => {
                                        e.stopPropagation();
                                        skipNext();
                                      }}
                                      onContextMenu={(e) => {
                                        e.preventDefault();
                                        e.stopPropagation();
                                        skipPrevious();
                                      }}
                                    >
                                      <div className="album-art-inner">
                                        {albumArtUrl ? (
                                          <img src={albumArtUrl} alt="Art" />
                                        ) : (
                                          <div className="album-art-placeholder">🎵</div>
                                        )}
                                        <div className="album-art-overlay">
                                          <div className="control-icon-small">
                                            {isPlaying ? <PauseIcon /> : <PlayIcon />}
                                          </div>
                                        </div>
                                      </div>
                                    </motion.button>
                                  ) : (
                                    isHovered && (
                                      <motion.div
                                        key="right-passive"
                                        className="passive-features-group"
                                        initial={{ opacity: 0, x: 10 }}
                                        animate={{ opacity: 1, x: 0 }}
                                        exit={{ opacity: 0, x: 10 }}
                                      >
                                        {settingsWeatherEnabled && temperature !== null && (
                                          <div className="passive-feature" title={weatherCondition}>
                                            <ThermometerIcon />
                                            <span className="label">{temperature}°{tempUnit === "fahrenheit" ? "F" : "C"}</span>
                                          </div>
                                        )}
                                        <div className="passive-feature">
                                          <BatteryIcon charging={isCharging} level={batteryLevel} threshold={lowBatteryThreshold} />
                                          <span className="label">{batteryLevel}%</span>
                                        </div>
                                      </motion.div>
                                    )
                                  )}
                                </AnimatePresence>
                              </div>
                            ) : (
                              /* Spacer to keep time centered if the OTHER side has content */
                              isMusicMode && settingsVisualizerEnabled && isPlaying && <div className="side-content right" />
                            )}
                          </motion.div>
                        )}
                      </AnimatePresence>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>


              {/* Calendar & Timer Split View */}
              <AnimatePresence>
                {settingsCalendarEnabled && isCalendarMode && (
                  <motion.div
                    className="calendar-timer-content split-view"
                    onClick={e => e.stopPropagation()} /* Block mode switches when clicking inside */
                    initial={{ opacity: 0, scale: 0.95 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, y: -40, scale: 0.9, filter: "blur(12px)", transition: { duration: 0.2 } }}
                    transition={{ type: "spring", stiffness: 400, damping: 30 }}
                  >
                    <div className="calendar-column">
                      <Calendar />
                    </div>

                    <div className="timer-column">
                      <div className="timer-section-new">
                        <div className="timer-display-large">
                          <span className="timer-time-large">{formatTimerTime(timerSeconds)}</span>
                        </div>

                        <div className="timer-controls-new">
                          <button onClick={toggleTimer} className="timer-btn primary">
                            {isTimerRunning ? 'Pause' : 'Start'}
                          </button>
                          <button onClick={resetTimer} className="timer-btn secondary">Reset</button>
                        </div>

                        <div className="timer-presets-new">
                          {[5, 15, 25, 50].map(mins => (
                            <button key={mins} onClick={() => startTimer(mins)} className="preset-btn-small">
                              {mins}m
                            </button>
                          ))}
                        </div>
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>
            </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

function Calendar() {
  const [date] = useState(new Date());

  const daysInMonth = (year: number, month: number) => new Date(year, month + 1, 0).getDate();
  const firstDayOfMonth = (year: number, month: number) => new Date(year, month, 1).getDay();

  const currentMonth = date.getMonth();
  const currentYear = date.getFullYear();
  const monthName = date.toLocaleString('default', { month: 'long' });

  const totalDays = daysInMonth(currentYear, currentMonth);
  const startDay = firstDayOfMonth(currentYear, currentMonth);
  const days = [];

  // Padding for start of month
  for (let i = 0; i < startDay; i++) {
    days.push(<div key={`empty-${i}`} className="calendar-day empty" />);
  }

  // Actual days
  const today = new Date().getDate();
  const isCurrentMonth = new Date().getMonth() === currentMonth && new Date().getFullYear() === currentYear;

  for (let i = 1; i <= totalDays; i++) {
    days.push(
      <div key={i} className={`calendar-day ${isCurrentMonth && i === today ? 'today' : ''}`}>
        {i}
      </div>
    );
  }

  return (
    <div className="calendar-container">
      <div className="calendar-header">
        <span className="month-year">{monthName} {currentYear}</span>
      </div>
      <div className="calendar-grid">
        {['S', 'M', 'T', 'W', 'T', 'F', 'S'].map((d, i) => (
          <div key={`${d}-${i}`} className="day-name">{d}</div>
        ))}
        {days}
      </div>
    </div>
  );
}

function PlayIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
      <path d="M8 5v14l11-7z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="currentColor">
      <rect x="6" y="4" width="4" height="16" />
      <rect x="14" y="4" width="4" height="16" />
    </svg>
  );
}

export default App;
