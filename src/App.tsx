import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback, useRef, memo } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./App.css";

// Simple SVG icons
function WifiIcon({ connected }: { connected: boolean }) {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" opacity={connected ? 1 : 0.4}>
      <path d="M5 12.55a11 11 0 0 1 14.08 0" />
      <path d="M1.42 9a16 16 0 0 1 21.16 0" />
      <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
      <line x1="12" y1="20" x2="12.01" y2="20" />
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

function TrayIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="3" y="3" width="6" height="6" rx="1" />
      <rect x="15" y="3" width="6" height="6" rx="1" />
      <rect x="15" y="15" width="6" height="6" rx="1" />
      <rect x="3" y="15" width="6" height="6" rx="1" />
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

const Visualizer = memo(function Visualizer({ isPlaying, bars = 5, height = 20 }: { isPlaying: boolean; bars?: number; height?: number }) {
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

  // Media state
  const [isPlaying, setIsPlaying] = useState(false);
  const [mediaInfo, setMediaInfo] = useState<MediaInfo>({
    title: "",
    artist: "",
    is_playing: false,
    has_media: false
  });
  const [albumArtUrl, setAlbumArtUrl] = useState<string | null>(null);
  const [volume, setVolume] = useState(0.5);
  const [wifiEnabled, setWifiEnabled] = useState(true);
  const [bluetoothEnabled, setBluetoothEnabled] = useState(true);
  const [currentBrightness, setCurrentBrightness] = useState(50);
  const [windowLabel, setWindowLabel] = useState<string>("");
  useEffect(() => {
    setWindowLabel(getCurrentWebviewWindow().label);
  }, []);
  const [isVisible, setIsVisible] = useState(true);
  const [isImpacted, setIsImpacted] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);

  const [notchMode, setNotchMode] = useState(() => localStorage.getItem("bloom-notch-mode") || "fixed");
  const [dockMode, setDockMode] = useState<'fixed' | 'auto-hide'>(() => {
    return (localStorage.getItem("bloom-dock-mode") as 'fixed' | 'auto-hide') || 'auto-hide';
  });
  const [dndActive, setDndActive] = useState(false);
  const [isNotchHovered, setIsNotchHovered] = useState(false);

  const [isEdgeHovered, setIsEdgeHovered] = useState(false);
  const [isOverlapped, setIsOverlapped] = useState(false);
  const [interactionState, setInteractionState] = useState<'active' | 'grace' | 'none'>('none');
  const bloomRef = useRef<HTMLDivElement>(null);

  const isAnyInteraction = isHovered || isNotchHovered || isEdgeHovered;
  const isHidden = notchMode === 'auto-hide' && isOverlapped && interactionState === 'none';

  useEffect(() => {
    if (isAnyInteraction) {
      setInteractionState('active');
    } else if (interactionState !== 'none') {
      setInteractionState('grace');
      const timer = setTimeout(() => setInteractionState('none'), 800);
      return () => clearTimeout(timer);
    }
  }, [isAnyInteraction]);

  useEffect(() => {
    if (windowLabel === 'main') {
      invoke('set_notch_hovered', { hovered: isNotchHovered }).catch(() => { });
    }
  }, [isNotchHovered, windowLabel]);

  useEffect(() => {
    const updateRect = () => {
      if (bloomRef.current && windowLabel === 'main') {
        const rect = bloomRef.current.getBoundingClientRect();
        invoke('update_notch_rect', {
          rect: {
            x: Math.round(rect.x),
            y: Math.round(rect.y),
            width: Math.round(rect.width),
            height: Math.round(rect.height)
          }
        }).catch(() => { });
      }
    };

    updateRect();
    window.addEventListener('resize', updateRect);
    const observer = new ResizeObserver(updateRect);
    if (bloomRef.current) observer.observe(bloomRef.current);

    return () => {
      window.removeEventListener('resize', updateRect);
      observer.disconnect();
    };
  }, [isExpanded, isHidden, windowLabel]);

  useEffect(() => {
    if (!windowLabel) return;

    // Only animate the main top-bar
    if (windowLabel !== 'main') {
      setIsReady(true);
      setIsImpacted(true);
      setIsExpanded(true);
      return;
    }

    const checkVisibility = async () => {
      try {
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
      } catch (e) { }
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
  const [settingsAmbienceEnabled, setSettingsAmbienceEnabled] = useState(() => localStorage.getItem("bloom-media-ambience-enabled") !== "false");
  const [settingsCornersEnabled, setSettingsCornersEnabled] = useState(() => localStorage.getItem("bloom-corners-enabled") === "true");
  const [tempUnit, setTempUnit] = useState(() => localStorage.getItem("bloom-temp-unit") || "celsius");

  useEffect(() => {
    if (!windowLabel) return;

    invoke("load_settings").then((settings: any) => {
      const getVal = (key: string, fallback: string | null = null) => {
        const val = settings[key];
        if (val !== undefined && val !== null) return String(val);
        const local = localStorage.getItem(key);
        if (local !== null) return local;
        return fallback;
      };

      setSettingsWeatherEnabled(getVal("bloom-weather-enabled", "true") !== "false");
      setSettingsCalendarEnabled(getVal("bloom-calendar-enabled", "true") !== "false");
      const viz = getVal("bloom-media-visualizer-enabled") ?? getVal("bloom-visualizer-enabled", "true");
      setSettingsVisualizerEnabled(viz !== "false");
      setSettingsAlbumArtEnabled(getVal("bloom-media-album-art-enabled", "true") !== "false");
      setSettingsAmbienceEnabled(getVal("bloom-media-ambience-enabled", "true") !== "false");
      setSettingsCornersEnabled(getVal("bloom-corners-enabled", "false") === "true");
      setTempUnit(getVal("bloom-temp-unit", "celsius") as string);

      const thresholdStr = getVal("bloom-low-battery-threshold", "20");
      if (thresholdStr) setLowBatteryThreshold(parseInt(thresholdStr as string));

      const nMode = getVal("bloom-notch-mode", "fixed");
      if (nMode) setNotchMode(nMode as string);

      if (windowLabel === 'main') {
        const firstRun = localStorage.getItem("bloom-first-run") === null;
        if (firstRun) {
          import("@tauri-apps/plugin-autostart").then(({ enable, isEnabled }) => {
            isEnabled().then(enabled => {
              if (!enabled) enable().catch(() => { });
            });
          });
          localStorage.setItem("bloom-first-run", "done");
        }
        const syncWindows = async () => {
          const dockEnabled = getVal("bloom-dock-enabled", "false") === "true";
          if (dockEnabled) {
            await invoke("init_dock", { mode: getVal("bloom-dock-mode", "auto-hide") });
          }
          await invoke("change_notch_mode", { mode: nMode });
          await invoke("sync_appbar");
        };

        // 1. Snappy initial sync (fast as possible)
        setTimeout(syncWindows, 400);

        // 2. Smooth layout corrections (only syncs position, doesn't re-toggle visibility)
        setTimeout(() => invoke("sync_appbar"), 1000);
        setTimeout(() => invoke("sync_appbar"), 2500);
      }
    }).catch(console.error);
  }, [windowLabel]); // eslint-disable-line react-hooks/exhaustive-deps

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
      if (key === "media-ambience-enabled") setSettingsAmbienceEnabled(value as boolean);
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
          if (value) {
            invoke("init_dock", { mode: localStorage.getItem("bloom-dock-mode") || "fixed" });
          } else {
            invoke("toggle_dock", { enable: false });
          }
          // Always re-sync topbar to prevent displacement when dock state changes
          setTimeout(() => invoke("sync_appbar"), 200);
        }
      }
      if (key === "dock-mode") {
        setDockMode(value);
        if (windowLabel === 'main') {
          invoke("change_dock_mode", { mode: value });
          setTimeout(() => invoke("sync_appbar"), 200);
        }
      }

      if (key === "notch-mode") {
        setNotchMode(value);
        if (windowLabel === 'main') {
          invoke("change_notch_mode", { mode: value });
        }
      }
    });

    const unlistenNotchOverlap = listen<boolean>("notch-overlap", (event) => {
      setIsOverlapped(event.payload);
    });

    const unlistenNotchEdgeHover = listen<boolean>("notch-edge-hover", (event) => {
      setIsEdgeHovered(event.payload);
    });

    return () => {
      unlistenVisibility.then(f => f());
      unlistenSettings.then(f => f());
      unlistenNotchOverlap.then(f => f());
      unlistenNotchEdgeHover.then(f => f());
    };
  }, [windowLabel]);

  // Bloom mode state: 'music', 'calendar', 'command-center', or 'status'
  const [bloomMode, setBloomMode] = useState<'music' | 'calendar' | 'command-center' | 'status'>('status');

  // Reset window height when state changes
  useEffect(() => {
    let targetHeight = 48;

    if (isExpanded) {
      if (bloomMode === 'calendar') {
        targetHeight = 320;
      } else if (bloomMode === 'command-center') {
        targetHeight = isHovered ? 230 : 48;
      } else if (bloomMode === 'status') {
        targetHeight = 48;
      } else if (isHovered) {
        targetHeight = mediaInfo.has_media ? 140 : 64;
      }
    }

    // Debounce the Tauri window resize to prevent race conditions during rapid HMR/scroll switches
    const delay = isExpanded ? 50 : 350; // Faster expansion, slightly delayed collapse for better UX
    const timeout = setTimeout(() => {
      invoke("set_window_height", { height: targetHeight });
    }, delay);

    return () => clearTimeout(timeout);
  }, [isHovered, bloomMode, isExpanded, mediaInfo.has_media]);

  const lastScrollTime = useRef(0);
  const handleWheel = (e: React.WheelEvent) => {
    const target = e.target as HTMLElement;
    if (target.closest('.calendar-grid') || target.closest('.timer-column')) {
      return;
    }

    if (!isHovered) return;

    const now = Date.now();
    if (now - lastScrollTime.current < 250) return;

    // Use absolute values to detect horizontal swipe gestures on trackpad
    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (Math.abs(delta) < 5) return; // Ignore tiny movements

    const modes: ('music' | 'command-center' | 'status' | 'calendar')[] = ['music', 'command-center', 'status', 'calendar'];
    const availableModes = modes.filter(m => m !== 'music' || mediaInfo.has_media);

    const currentIndex = availableModes.indexOf(bloomMode);
    if (currentIndex === -1) return;

    if (delta > 0) {
      const nextIndex = (currentIndex + 1) % availableModes.length;
      setBloomMode(availableModes[nextIndex]);
      lastScrollTime.current = now;
    } else if (delta < 0) {
      const prevIndex = (currentIndex - 1 + availableModes.length) % availableModes.length;
      setBloomMode(availableModes[prevIndex]);
      lastScrollTime.current = now;
    }
  };



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

  // Auto-switch back from music if music stops for 5 seconds
  useEffect(() => {
    let timer: any;
    if (!isPlaying && bloomMode === 'music') {
      timer = setTimeout(() => {
        setBloomMode('status');
      }, 5000);
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

  // Listen for Volume Changes
  useEffect(() => {
    const unlisten = listen<{ volume: number; is_muted: boolean }>("volume-change", (event) => {
      setVolume(event.payload.volume);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // Load wifi/bluetooth/volume/brightness state on mount
  useEffect(() => {
    invoke<boolean>("get_wifi_state").then(setWifiEnabled).catch(() => {});
    invoke<boolean>("get_bluetooth_state").then(setBluetoothEnabled).catch(() => {});
    invoke<number>("get_volume").then(setVolume).catch(() => {});
    invoke<number>("get_brightness").then(setCurrentBrightness).catch(() => {});
  }, []);

  // Listen for brightness changes
  useEffect(() => {
    const unlisten = listen<{ brightness: number }>("brightness-change", (event) => {
      setCurrentBrightness(event.payload.brightness);
    });
    return () => { unlisten.then(fn => fn()); };
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
  /* Unused saveAndBroadcast removed to fix TS build error */
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

  const handleVolumeChange = useCallback(async (newVol: number) => {
    setVolume(newVol);
    try {
      await invoke("set_volume", { volume: newVol });
    } catch (e) {
      console.error("Failed to set volume:", e);
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

  // WiFi toggle
  const toggleWifi = useCallback(async () => {
    const newState = !wifiEnabled;
    setWifiEnabled(newState);
    try {
      await invoke("set_wifi_state", { enabled: newState });
    } catch (e) {
      setWifiEnabled(!newState);
      console.error("Failed to toggle WiFi:", e);
    }
  }, [wifiEnabled]);

  // Bluetooth toggle
  const toggleBluetooth = useCallback(async () => {
    const newState = !bluetoothEnabled;
    setBluetoothEnabled(newState);
    try {
      await invoke("set_bluetooth_state", { enabled: newState });
    } catch (e) {
      setBluetoothEnabled(!newState);
      console.error("Failed to toggle Bluetooth:", e);
    }
  }, [bluetoothEnabled]);


  // Brightness change
  const handleBrightnessChange = useCallback(async (newVal: number) => {
    setCurrentBrightness(newVal);
    try {
      await invoke("set_brightness", { brightness: newVal });
    } catch (e) {
      console.error("Failed to set brightness:", e);
    }
  }, []);

  // Open system tray (unhide taskbar and invoke Win+B)
  const openSystemTray = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await invoke("open_system_tray");
    } catch (e) {
      console.error("Failed to open system tray:", e);
    }
  }, []);

  const openSettingsWindow = useCallback(async () => {
    try {
      await invoke("open_settings_window");
    } catch (e) {
      console.error("Failed to open settings window:", e);
    }
  }, []);

  const handleWifiRightClick = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    openWifiSettings();
  }, [openWifiSettings]);

  const toggleDockModeSetting = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    const nextMode = dockMode === "fixed" ? "auto-hide" : "fixed";
    setDockMode(nextMode);
    localStorage.setItem("bloom-dock-mode", nextMode);
    try {
      await invoke("change_dock_mode", { mode: nextMode });
      await invoke("broadcast_setting", { key: "dock-mode", value: nextMode });
    } catch (err) {
      console.error("Failed to change dock mode:", err);
    }
  }, [dockMode]);

  const toggleNotchModeSetting = useCallback(async (e: React.MouseEvent) => {
    e.stopPropagation();
    const nextMode = notchMode === "fixed" ? "auto-hide" : "fixed";
    setNotchMode(nextMode);
    localStorage.setItem("bloom-notch-mode", nextMode);
    try {
      await invoke("change_notch_mode", { mode: nextMode });
      await invoke("broadcast_setting", { key: "notch-mode", value: nextMode });
    } catch (err) {
      console.error("Failed to change notch mode:", err);
    }
  }, [notchMode]);

  const handleBluetoothRightClick = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    e.stopPropagation();
    invoke("open_bluetooth_settings");
  }, []);



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
    if (bloomMode === 'command-center' && isHovered) return 350;
    if (bloomMode === 'status' && isHovered) return 280;
    if (isMusicMode && isHovered) return 340;
    if ((showPowerPulse || showLowBatteryPulse) && !isHovered) return 200;

    let w = 140;
    if (isMusicMode) {
      w = 140;
      if (settingsVisualizerEnabled && isPlaying) w += 30;
      if (settingsAlbumArtEnabled) w += 30;

      if (isHovered) {
        w += 60;
      }
    }

    return w;
  };

  const getDynamicHeight = () => {
    if (!isExpanded || !isVisible || isHidden) {
      return isImpacted ? 28.9 : 44.2;
    }
    if (bloomMode === 'calendar') return 280;
    if (bloomMode === 'command-center') return isHovered ? 230 : 34;
    if (bloomMode === 'status') return 34;
    if (isMusicMode && isHovered) return 120;
    return 34;
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
        ref={bloomRef}
        className={`bloom ${isHovered ? 'expanded' : ''} ${isImpacted ? 'is-impacted' : ''}`}
        onMouseEnter={() => setIsNotchHovered(true)}
        onMouseLeave={() => setIsNotchHovered(false)}
        onWheel={handleWheel}
        initial={{ y: 250, width: 30.6, height: 44.2, borderTopLeftRadius: 18, borderTopRightRadius: 18, borderBottomLeftRadius: 18, borderBottomRightRadius: 18, scaleX: 1, scaleY: 1, opacity: 0 }}
        animate={{
          y: !isReady ? 250 : (isVisible ? (isHidden ? -100 : 0) : -150),
          width: !isReady ? 34 : (isExpanded && isVisible && !isHidden ? getDynamicWidth() : (isImpacted ? 39.1 : 30.6)),
          height: !isReady ? 34 : getDynamicHeight(),
          opacity: isVisible ? 1 : 0,
          scaleX: 1,
          scaleY: 1,
          borderTopLeftRadius: isImpacted ? 0 : 18,
          borderTopRightRadius: isImpacted ? 0 : 18,
          borderBottomLeftRadius: 18,
          borderBottomRightRadius: 18,
          filter: isVisible ? "blur(0px)" : "blur(8px)",
          pointerEvents: isVisible ? 'auto' : 'none'
        }}
        onClick={(e) => {
          e.stopPropagation();
        }}
        onHoverStart={() => {
          setIsHovered(true);
          setBloomMode(mediaInfo.has_media && isPlaying ? 'music' : 'status');
        }}
        onHoverEnd={() => {
          setIsHovered(false);
          if (bloomMode === 'command-center' || bloomMode === 'calendar' || bloomMode === 'status') {
            setBloomMode(mediaInfo.has_media && isPlaying ? 'music' : 'status');
          }
        }}
        style={{ originY: 0 }}
        transition={{
          width: { type: "spring", stiffness: 400, damping: 28 },
          height: { type: "spring", stiffness: 450, damping: 26 },
          y: { type: "spring", stiffness: 550, damping: 45, mass: 0.8, restDelta: 0.001 },
          opacity: { duration: 0.2 },
          borderTopLeftRadius: { type: "spring", stiffness: 1000, damping: 40 },
          borderTopRightRadius: { type: "spring", stiffness: 1000, damping: 40 },
          default: { type: "spring", stiffness: 500, damping: 30, mass: 1 }
        }}
      >

        {isMusicMode && settingsAmbienceEnabled && albumArtUrl && isHovered && !isCalendarMode && (
          <div className="notch-ambient-glow">
            <img src={albumArtUrl} alt="" />
          </div>
        )}
        <AnimatePresence mode="wait">
          {isExpanded && (
            <motion.div
              key="bloom-content"
              initial={{ opacity: 0, scale: 0.9 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.9 }}
              transition={{ duration: 0.2 }}
              style={{ width: '100%', height: '100%', display: 'flex', flexDirection: 'column', alignItems: 'center', position: 'relative', overflow: 'hidden', borderRadius: 'inherit' }}
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
                        <div className="track-header-row">
                          <div className="track-info-middle">
                            <span className="premium-title">{mediaInfo.title}</span>
                            <span className="premium-artist">{mediaInfo.artist}</span>
                          </div>
                          <div className="header-visualizer">
                            <Visualizer isPlaying={isPlaying} bars={5} height={18} />
                          </div>
                        </div>

                        <div className="controls-row-sleek">
                          <motion.button
                            className="sleek-btn previous-btn"
                            onClick={(e) => { e.stopPropagation(); skipPrevious(); }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.9 }}
                            transition={{ type: "spring", stiffness: 400, damping: 25 }}
                          >
                            <svg width="24" height="12" viewBox="0 0 66 32" fill="currentColor">
                              <g transform="scale(-1,1) translate(-66,0)">
                                <path d="M 7.54 0.06 C 8.12 0.06 8.78 0.36 9.23 0.64 L 31.66 13.83 C 32.11 14.09 32.48 14.45 32.63 14.93 L 32.63 2.55 C 32.63 0.81 33.68 0.06 34.71 0.06 C 35.27 0.06 35.94 0.36 36.39 0.64 L 58.84 13.83 C 59.46 14.2 59.91 14.78 59.91 15.59 C 59.91 16.41 59.51 16.9 58.84 17.31 L 36.39 30.5 C 35.9 30.78 35.27 31.08 34.71 31.08 C 33.68 31.08 32.63 30.33 32.63 28.57 L 32.63 16.26 C 32.48 16.71 32.14 17.03 31.66 17.31 L 9.23 30.5 C 8.74 30.78 8.12 31.08 7.54 31.08 C 6.5 31.08 5.47 30.33 5.47 28.57 L 5.47 2.55 C 5.47 0.81 6.5 0.06 7.54 0.06 Z" />
                              </g>
                            </svg>
                          </motion.button>

                          <motion.button
                            className="sleek-btn play-pause-btn-floating"
                            onClick={(e) => { e.stopPropagation(); togglePlayPause(); }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.95 }}
                            transition={{ type: "spring", stiffness: 400, damping: 25 }}
                          >
                            <AnimatePresence mode="wait" initial={false}>
                              <motion.div
                                key={isPlaying ? "pause" : "play"}
                                initial={{ opacity: 0, scale: 0.8 }}
                                animate={{ opacity: 1, scale: 1 }}
                                exit={{ opacity: 0, scale: 0.8 }}
                                transition={{ duration: 0.15 }}
                                style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}
                              >
                                {isPlaying ? (
                                  <svg width="24" height="24" viewBox="0 0 33 35" fill="currentColor">
                                    <path d="M 7.390625,0.81640625 L 11.880859375,0.81640625 C 13.234375,0.81640625 13.771484375,1.375 13.771484375,2.728515625 L 13.771484375,31.560546875 C 13.771484375,32.935546875 13.234375,33.47265625 11.880859375,33.47265625 L 7.390625,33.47265625 C 6.015625,33.47265625 5.478515625,32.935546875 5.478515625,31.560546875 L 5.478515625,2.728515625 C 5.478515625,1.375 6.015625,0.81640625 7.390625,0.81640625 Z M 20.8828125,0.81640625 L 25.373046875,0.81640625 C 26.748046875,0.81640625 27.28515625,1.375 27.28515625,2.728515625 L 27.28515625,31.560546875 C 27.28515625,32.935546875 26.748046875,33.47265625 25.373046875,33.47265625 L 20.8828125,33.47265625 C 19.529296875,33.47265625 18.9921875,32.935546875 18.9921875,31.560546875 L 18.9921875,2.728515625 C 18.9921875,1.375 19.529296875,0.81640625 20.8828125,0.81640625 Z" />
                                  </svg>
                                ) : (
                                  <svg width="28" height="28" viewBox="0 0 42 41" fill="currentColor" style={{ marginLeft: '2px' }}>
                                    <path d="M 6.91796875,2.298828125 C 7.34765625,2.298828125 7.669921875,2.40625 8.20703125,2.728515625 L 35.083984375,18.583984375 C 35.8359375,18.9921875 36.287109375,19.400390625 36.287109375,20.087890625 C 36.287109375,20.75390625 35.8359375,21.162109375 35.083984375,21.591796875 L 8.20703125,37.447265625 C 7.669921875,37.76953125 7.34765625,37.876953125 6.91796875,37.876953125 C 6.05859375,37.876953125 5.478515625,37.25390625 5.478515625,36.1796875 L 5.478515625,3.99609375 C 5.478515625,2.921875 6.05859375,2.298828125 6.91796875,2.298828125 Z" />
                                  </svg>
                                )}
                              </motion.div>
                            </AnimatePresence>
                          </motion.button>

                          <motion.button
                            className="sleek-btn next-btn"
                            onClick={(e) => { e.stopPropagation(); skipNext(); }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.9 }}
                            transition={{ type: "spring", stiffness: 400, damping: 25 }}
                          >
                            <svg width="24" height="12" viewBox="0 0 66 32" fill="currentColor">
                              <path d="M 7.54 0.06 C 8.12 0.06 8.78 0.36 9.23 0.64 L 31.66 13.83 C 32.11 14.09 32.48 14.45 32.63 14.93 L 32.63 2.55 C 32.63 0.81 33.68 0.06 34.71 0.06 C 35.27 0.06 35.94 0.36 36.39 0.64 L 58.84 13.83 C 59.46 14.2 59.91 14.78 59.91 15.59 C 59.91 16.41 59.51 16.9 58.84 17.31 L 36.39 30.5 C 35.9 30.78 35.27 31.08 34.71 31.08 C 33.68 31.08 32.63 30.33 32.63 28.57 L 32.63 16.26 C 32.48 16.71 32.14 17.03 31.66 17.31 L 9.23 30.5 C 8.74 30.78 8.12 31.08 7.54 31.08 C 6.5 31.08 5.47 30.33 5.47 28.57 L 5.47 2.55 C 5.47 0.81 6.5 0.06 7.54 0.06 Z" />
                            </svg>
                          </motion.button>
                        </div>

                        <div className="volume-slider-container">
                          <VolumeLowIcon />
                          <div className="slider-track-premium">
                            <input
                              type="range"
                              min="0"
                              max="1"
                              step="0.01"
                              value={volume}
                              onChange={(e) => handleVolumeChange(parseFloat(e.target.value))}
                              onPointerDown={(e) => e.stopPropagation()}
                              onClick={(e) => e.stopPropagation()}
                              className="premium-slider"
                            />
                            <div className="slider-progress-fill" style={{ width: `${volume * 100}%` }} />
                          </div>
                          <VolumeHighIcon />
                        </div>
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
                            {/* Left: visualizer (music) or weather (command-center, calendar) */}
                            {isMusicMode && settingsVisualizerEnabled ? (
                              <div className="side-content left">
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
                              </div>
                            ) : (!isMusicMode && isHovered && settingsWeatherEnabled && temperature !== null) ? (
                              <div className="side-content left">
                                <motion.div
                                  key="left-weather"
                                  className="passive-features-group"
                                  initial={{ opacity: 0 }}
                                  animate={{ opacity: 1 }}
                                  exit={{ opacity: 0 }}
                                >
                                  <div className="passive-feature" title={weatherCondition}>
                                    <ThermometerIcon />
                                    <span className="label">{temperature}°{tempUnit === "fahrenheit" ? "F" : "C"}</span>
                                  </div>
                                </motion.div>
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

                            {/* Right: album art (music) or battery (command-center, calendar) */}
                            {isMusicMode && settingsAlbumArtEnabled ? (
                              <div className="side-content right">
                                <AnimatePresence mode="wait">
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
                                </AnimatePresence>
                              </div>
                            ) : (!isMusicMode && isHovered) ? (
                              <div className="side-content right">
                                <div className="passive-features-group">
                                  <div className="passive-feature">
                                    <BatteryIcon charging={isCharging} level={batteryLevel} threshold={lowBatteryThreshold} />
                                    <span className="label">{batteryLevel}%</span>
                                  </div>
                                </div>
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

              {/* Command Center Panel */}
              <AnimatePresence>
                {bloomMode === 'command-center' && (
                  <motion.div
                    className="command-center-content-minimal"
                    onClick={e => e.stopPropagation()}
                    initial={{ opacity: 0, scale: 0.98 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.98, filter: "blur(4px)", transition: { duration: 0.15 } }}
                    transition={{ type: "spring", stiffness: 400, damping: 30 }}
                  >
                    {/* Pills Grid */}
                    <div className="cc-pills-grid">
                      {/* Wi-Fi Pill */}
                      <div 
                        className={`cc-pill-tile ${wifiEnabled ? 'active' : ''}`}
                        onClick={(e) => { e.stopPropagation(); toggleWifi(); }}
                        onContextMenu={handleWifiRightClick}
                        title="Left-click to toggle, Right-click for Settings"
                      >
                        <div className="cc-pill-icon-wrapper">
                          <WifiIcon connected={wifiEnabled} />
                        </div>
                        <div className="cc-pill-info">
                          <span className="cc-pill-title">Wi-Fi</span>
                          <span className="cc-pill-status">{wifiEnabled ? 'Connected' : 'Off'}</span>
                        </div>
                      </div>

                      {/* Dock Mode Pill */}
                      <div 
                        className={`cc-pill-tile ${dockMode === 'fixed' ? 'active' : ''}`}
                        onClick={toggleDockModeSetting}
                        title="Toggle Dock fixed / auto-hide"
                      >
                        <div className="cc-pill-icon-wrapper">
                          <DockIcon />
                        </div>
                        <div className="cc-pill-info">
                          <span className="cc-pill-title">Dock Mode</span>
                          <span className="cc-pill-status">{dockMode === 'fixed' ? 'Fixed' : 'Auto Hide'}</span>
                        </div>
                      </div>

                      {/* Bluetooth Pill */}
                      <div 
                        className={`cc-pill-tile ${bluetoothEnabled ? 'active' : ''}`}
                        onClick={(e) => { e.stopPropagation(); toggleBluetooth(); }}
                        onContextMenu={handleBluetoothRightClick}
                        title="Left-click to toggle, Right-click for Settings"
                      >
                        <div className="cc-pill-icon-wrapper">
                          <BluetoothIcon />
                        </div>
                        <div className="cc-pill-info">
                          <span className="cc-pill-title">Bluetooth</span>
                          <span className="cc-pill-status">{bluetoothEnabled ? 'On' : 'Off'}</span>
                        </div>
                      </div>

                      {/* Notch Mode Pill */}
                      <div 
                        className={`cc-pill-tile ${notchMode === 'fixed' ? 'active' : ''}`}
                        onClick={toggleNotchModeSetting}
                        title="Toggle Notch fixed / auto-hide"
                      >
                        <div className="cc-pill-icon-wrapper">
                          <NotchIcon />
                        </div>
                        <div className="cc-pill-info">
                          <span className="cc-pill-title">Notch Mode</span>
                          <span className="cc-pill-status">{notchMode === 'fixed' ? 'Fixed' : 'Auto Hide'}</span>
                        </div>
                      </div>
                    </div>

                    {/* Circular Actions Row */}
                    <div className="cc-circular-actions-row">
                      <button 
                        className={`cc-circular-btn ${dndActive ? 'active' : ''}`}
                        onClick={(e) => { e.stopPropagation(); setDndActive(prev => !prev); }}
                        title={`Focus / DND: ${dndActive ? 'On' : 'Off'}`}
                      >
                        <MoonIcon />
                      </button>
                      <button 
                        className="cc-circular-btn" 
                        onClick={(e) => { e.stopPropagation(); openSystemTray(e); }}
                        title="System Tray"
                      >
                        <TrayIcon />
                      </button>
                      <button 
                        className="cc-circular-btn" 
                        onClick={(e) => { e.stopPropagation(); invoke("open_notification_center"); }}
                        title="Notification Center"
                      >
                        <BellIcon />
                      </button>
                      <button 
                        className="cc-circular-btn" 
                        onClick={(e) => { e.stopPropagation(); openSettingsWindow(); }}
                        title="Bloom Settings"
                      >
                        <SettingsIcon />
                      </button>
                      <button 
                        className="cc-circular-btn" 
                        onClick={(e) => { e.stopPropagation(); invoke("restart_bloom"); }}
                        title="Restart Bloom"
                      >
                        <ReloadIcon />
                      </button>
                    </div>

                    {/* Classic Sliders Area */}
                    <div className="cc-classic-sliders-area">
                      {/* Volume Slider */}
                      <div className="cc-classic-slider-row">
                        <div className="cc-classic-slider-label">
                          <VolumeLowIcon />
                          <span>Volume</span>
                        </div>
                        <div className="cc-classic-slider-track">
                          <input
                            type="range"
                            min="0"
                            max="1"
                            step="0.01"
                            value={volume}
                            onChange={(e) => handleVolumeChange(parseFloat(e.target.value))}
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={(e) => e.stopPropagation()}
                            className="cc-classic-input"
                          />
                          <div className="cc-classic-fill" style={{ width: `${volume * 100}%` }} />
                        </div>
                        <span className="cc-classic-percentage">{Math.round(volume * 100)}%</span>
                      </div>

                      {/* Brightness Slider */}
                      <div className="cc-classic-slider-row">
                        <div className="cc-classic-slider-label">
                          <BrightnessLowIcon />
                          <span>Brightness</span>
                        </div>
                        <div className="cc-classic-slider-track">
                          <input
                            type="range"
                            min="0"
                            max="100"
                            step="1"
                            value={currentBrightness}
                            onChange={(e) => handleBrightnessChange(parseInt(e.target.value))}
                            onPointerDown={(e) => e.stopPropagation()}
                            onClick={(e) => e.stopPropagation()}
                            className="cc-classic-input"
                          />
                          <div className="cc-classic-fill" style={{ width: `${currentBrightness}%` }} />
                        </div>
                        <span className="cc-classic-percentage">{currentBrightness}%</span>
                      </div>
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
                    initial={{ opacity: 0, scale: 0.98 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.98, filter: "blur(4px)", transition: { duration: 0.15 } }}
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
    <svg width="14" height="14" viewBox="0 0 42 41" fill="currentColor" style={{ marginLeft: '1px' }}>
      <path d="M 6.91796875,2.298828125 C 7.34765625,2.298828125 7.669921875,2.40625 8.20703125,2.728515625 L 35.083984375,18.583984375 C 35.8359375,18.9921875 36.287109375,19.400390625 36.287109375,20.087890625 C 36.287109375,20.75390625 35.8359375,21.162109375 35.083984375,21.591796875 L 8.20703125,37.447265625 C 7.669921875,37.76953125 7.34765625,37.876953125 6.91796875,37.876953125 C 6.05859375,37.876953125 5.478515625,37.25390625 5.478515625,36.1796875 L 5.478515625,3.99609375 C 5.478515625,2.921875 6.05859375,2.298828125 6.91796875,2.298828125 Z" />
    </svg>
  );
}

function PauseIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 33 35" fill="currentColor">
      <path d="M 7.390625,0.81640625 L 11.880859375,0.81640625 C 13.234375,0.81640625 13.771484375,1.375 13.771484375,2.728515625 L 13.771484375,31.560546875 C 13.771484375,32.935546875 13.234375,33.47265625 11.880859375,33.47265625 L 7.390625,33.47265625 C 6.015625,33.47265625 5.478515625,32.935546875 5.478515625,31.560546875 L 5.478515625,2.728515625 C 5.478515625,1.375 6.015625,0.81640625 7.390625,0.81640625 Z M 20.8828125,0.81640625 L 25.373046875,0.81640625 C 26.748046875,0.81640625 27.28515625,1.375 27.28515625,2.728515625 L 27.28515625,31.560546875 C 27.28515625,32.935546875 26.748046875,33.47265625 25.373046875,33.47265625 L 20.8828125,33.47265625 C 19.529296875,33.47265625 18.9921875,32.935546875 18.9921875,31.560546875 L 18.9921875,2.728515625 C 18.9921875,1.375 19.529296875,0.81640625 20.8828125,0.81640625 Z" />
    </svg>
  );
}

function VolumeLowIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="currentColor" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
      <path d="M11 5L6 9H2v6h4l5 4V5z" />
    </svg>
  );
}

function VolumeHighIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
      <path d="M11 5L6 9H2v6h4l5 4V5z" fill="currentColor" />
      <path d="M15.54 8.46a5 5 0 0 1 0 7.07" strokeWidth="2.5" />
      <path d="M19.07 4.93a10 10 0 0 1 0 14.14" strokeWidth="2.5" />
    </svg>
  );
}

function BluetoothIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6.5 6.5l11 11L12 23V1l5.5 5.5-11 11" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.9 }}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function BrightnessLowIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" style={{ opacity: 0.5 }}>
      <circle cx="12" cy="12" r="5" fill="currentColor" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

function DockIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <rect x="2" y="14" width="20" height="8" rx="2" />
      <line x1="6" y1="18" x2="6.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
      <line x1="10" y1="18" x2="10.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
      <line x1="14" y1="18" x2="14.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
      <line x1="18" y1="18" x2="18.01" y2="18" strokeWidth="3.5" strokeLinecap="round" />
    </svg>
  );
}

function NotchIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 3h16a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />
      <path d="M9 9v4a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2V9" />
    </svg>
  );
}

function BellIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9z" />
      <path d="M13.73 21a2 2 0 0 1-3.46 0" />
    </svg>
  );
}

function ReloadIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round">
      <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
    </svg>
  );
}

export default App;
