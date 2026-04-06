import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback, useRef } from "react";
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
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
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

function BatteryIcon({ charging, level }: { charging: boolean; level: number }) {
  const fillWidth = (level / 100) * 10;

  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="7" width="16" height="10" rx="2" ry="2" />
      <line x1="22" y1="11" x2="22" y2="13" />
      {charging ? (
        <path d="M11 10v4h2l-1 4" stroke="currentColor" fill="none" />
      ) : (
        <rect x="4" y="9" width={fillWidth} height="6" fill="currentColor" stroke="none" />
      )}
    </svg>
  );
}

// Horizontal music visualizer bars (like iPhone Dynamic Island)
function Visualizer({ isPlaying, audioData }: { isPlaying: boolean; audioData: number[] }) {
  return (
    <div className="visualizer-horizontal">
      {audioData.map((value, i) => (
        <motion.div
          key={i}
          className="bar-horizontal"
          animate={{
            scaleY: isPlaying ? value : 0.1,
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
}

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

  // Battery state
  const [batteryLevel, setBatteryLevel] = useState(100);
  const [isCharging, setIsCharging] = useState(false);

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

  // Settings state
  const [settingsWeatherEnabled, setSettingsWeatherEnabled] = useState(() => localStorage.getItem("bloom-weather-enabled") !== "false");
  const [settingsCalendarEnabled, setSettingsCalendarEnabled] = useState(() => localStorage.getItem("bloom-calendar-enabled") !== "false");
  const [settingsVisualizerEnabled, setSettingsVisualizerEnabled] = useState(() => localStorage.getItem("bloom-media-visualizer-enabled") !== "false");
  const [settingsAlbumArtEnabled, setSettingsAlbumArtEnabled] = useState(() => localStorage.getItem("bloom-media-album-art-enabled") !== "false");
  const [settingsMediaDetailsEnabled, setSettingsMediaDetailsEnabled] = useState(() => localStorage.getItem("bloom-media-details-enabled") !== "false");
  const [settingsCornersMode, setSettingsCornersMode] = useState(() => localStorage.getItem("bloom-corners-mode") || "top");
  const [tempUnit, setTempUnit] = useState(() => localStorage.getItem("bloom-temp-unit") || "celsius");

  useEffect(() => {
    // On startup, sync the corners window visibility
    if (windowLabel === 'main') {
      invoke("toggle_corners_window", { mode: settingsCornersMode });
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
      console.log("App received settings-changed:", event.payload);
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
      if (key === "corners-mode") {
        setSettingsCornersMode(value);
        if (windowLabel === 'main') {
           invoke("toggle_corners_window", { mode: value });
        }
      }
    });

    return () => {
      unlistenVisibility.then(f => f());
      unlistenSettings.then(f => f());
    };
  }, []);
  // Audio visualization state
  const [audioData, setAudioData] = useState<number[]>(new Array(5).fill(0.18));

  // New bloom mode state: 'status', 'music', or 'calendar'
  const [bloomMode, setBloomMode] = useState<'status' | 'music' | 'calendar'>('status');
  const [isMuted] = useState(false);

  // Reset window height when state changes
  useEffect(() => {
    let timeout: any;
    if (bloomMode === 'calendar') {
      invoke("set_window_height", { height: 275 });
    } else if (isHovered) {
      invoke("set_window_height", { height: 240 });
    } else {
      // Wait for the spring animation to finish before snapping the OS window bounds
      timeout = setTimeout(() => {
        invoke("set_window_height", { height: 40 });
      }, 400);
    }
    return () => clearTimeout(timeout);
  }, [isHovered, bloomMode === 'calendar']);

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
      console.log("Auto-switching to music mode. NewTrack:", isNewTrackWhilePlaying, "NewPlay:", justStartedPlaying);
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
        console.log("Battery API not supported");
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
        console.log(`Weather: Received temperature ${temp} for ${latitude}, ${longitude}`);
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
          console.log(`Weather: Manual coordinates found: ${savedLat}, ${savedLon}`);
          await fetchWeather(parseFloat(savedLat), parseFloat(savedLon));
          return;
        }

        console.log("Weather: Fetching location via Rust...");
        const jsonStr: string = await invoke("get_location_from_ip");
        const data = JSON.parse(jsonStr);
        const lat = data.latitude || data.lat;
        const lon = data.longitude || data.lon;
        if (lat && lon) {
          console.log(`Weather: Location found (${data.city}): ${lat}, ${lon}`);
          await fetchWeather(lat, lon);
        } else {
          throw new Error("Rust location data missing lat/lon fields");
        }
      } catch (e) {
        console.warn("Weather: Rust location fetch failed, falling back to Delhi.", e);
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
        // Deep-ish check for changes to prevent unnecessary re-renders/glitches
        const isSame = prev.title === info.title && 
                       prev.artist === info.artist && 
                       prev.is_playing === info.is_playing && 
                       prev.has_media === info.has_media;
        
        if (isSame) return prev;
        
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

  // Audio visualization - receive data from backend
  useEffect(() => {
    const unlisten = listen<{ frequencies: number[] }>("audio-visualization", (event) => {
      const frequencies = event.payload.frequencies;
      // Just use the smoothed values from backend directly
      setAudioData(frequencies);
    });

    return () => {
      unlisten.then(fn => fn());
    };
  }, []);

  // Reset visualization when not playing
  useEffect(() => {
    if (!isPlaying) {
      setAudioData(new Array(5).fill(0.18));
    }
  }, [isPlaying]);

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
        console.log("Found existing settings window, showing...");
        await settingsWebview.show();
        await settingsWebview.unminimize();
        await settingsWebview.setFocus();
      } else {
        console.log("Settings window not found, creating new one...");
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
    if (isPlaying) {
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
    
    // Narrow base is 140. 
    // - Visualizer adds ~30px.
    // - Album art adds ~30px.
    // - Hover/expanded state adds more.
    
    let w = 140;
    if (isMusicMode) {
      w = 140;
      if (settingsVisualizerEnabled && isPlaying) w += 30;
      if (settingsAlbumArtEnabled) w += 30;
      
      if (isHovered) {
        // Expanded music mode
        w += 60; 
      }
    } else if (isHovered) {
      w = 320;
    }
    
    return w;
  };

  const isCalendarMode = bloomMode === 'calendar';

  if (windowLabel === 'bottom-corners') {
    if (settingsCornersMode !== 'all') return null;
    return (
      <div className="screen" style={{ alignItems: 'flex-end' }}>
        <AnimatePresence>
          {isVisible && (
            <>
              <motion.div 
                className="screen-corner bottom-left" 
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, filter: "blur(0px)" }}
                exit={{ opacity: 0, filter: "blur(10px)" }}
              />
              <motion.div 
                className="screen-corner bottom-right" 
                initial={{ opacity: 0 }}
                animate={{ opacity: 1, filter: "blur(0px)" }}
                exit={{ opacity: 0, filter: "blur(10px)" }}
              />
            </>
          )}
        </AnimatePresence>
      </div>
    );
  }

  if (windowLabel === 'bottom-corners') return null;

  return (
    <div className="screen" style={{ overflow: 'hidden' }}>
      {/* Screen Corners (Top) */}
      <AnimatePresence>
        {isVisible && settingsCornersMode !== 'none' && (
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
        className={`bloom ${isHovered ? 'expanded' : ''}`}
        initial={{ width: 140, y: -100 }}
        animate={{
          width: getDynamicWidth(),
          height: isCalendarMode
            ? 275
            : isHovered && mediaInfo.has_media && isPlaying ? 64 : 36,
          y: isVisible ? 0 : -80,
          opacity: isVisible ? 1 : 0,
          scale: isVisible ? 1 : 0.95,
          filter: isVisible ? "blur(0px)" : "blur(8px)"
        }}
        onClick={() => {
          // Only toggle if not in calendar mode
          if (!isCalendarMode) handleBloomClick();
        }}
        onHoverStart={() => setIsHovered(true)}
        onHoverEnd={() => {
          setIsHovered(false);
          if (isCalendarMode) {
            setBloomMode(mediaInfo.has_media && isPlaying ? 'music' : 'status');
          }
        }}
        style={{ originY: 0 }}
        transition={{ 
          y: { type: "spring", stiffness: 400, damping: 40, mass: 0.8, restDelta: 0.001 },
          default: { type: "spring", stiffness: 400, damping: 25, mass: 0.8 }
        }}
      >
        <div className="main-row">
          {/* Left: visualizer (music) or wifi+notifs (status) */}
          {(isMusicMode && settingsVisualizerEnabled && isPlaying) || (!isMusicMode && isHovered) ? (
            <div className="side-content left">
              <AnimatePresence mode="wait">
                {isMusicMode ? (
                  <AnimatePresence>
                    {isPlaying && settingsVisualizerEnabled && (
                      <motion.div
                        key="visualizer"
                        initial={{ scale: 0.8, opacity: 0 }}
                        animate={{ scale: 1, opacity: 1 }}
                        exit={{ scale: 0.8, opacity: 0 }}
                      >
                        <Visualizer isPlaying={isPlaying} audioData={audioData} />
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
                        <BatteryIcon charging={isCharging} level={batteryLevel} />
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
        </div>

        {/* Bottom row - Song details (only on hover when playing) */}
        <AnimatePresence>
          {isHovered && isPlaying && mediaInfo.has_media && !isCalendarMode && settingsMediaDetailsEnabled && (
            <motion.div
              className="bottom-row"
              initial={{ opacity: 0, height: 0, scale: 0.95, filter: "blur(4px)" }}
              animate={{ opacity: 1, height: 28, scale: 1, filter: "blur(0px)" }}
              exit={{ opacity: 0, height: 0, y: -15, scale: 0.95, filter: "blur(8px)" }}
              transition={{
                default: { type: "spring", stiffness: 400, damping: 25, mass: 0.8 },
                opacity: { duration: 0.2, ease: "easeOut" },
                filter: { duration: 0.2, ease: "easeOut" }
              }}
            >
              <MarqueeText title={mediaInfo.title} artist={mediaInfo.artist} />
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
