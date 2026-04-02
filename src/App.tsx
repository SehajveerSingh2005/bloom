import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
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
  
  // Audio visualization state
  const [audioData, setAudioData] = useState<number[]>(new Array(5).fill(0.18));

  // Notification state
  const [isMuted] = useState(false);

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
    return () => clearInterval(interval);
  }, []);

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
    const fetchWeather = async () => {
      try {
        // Get location from browser (if permitted)
        if ("geolocation" in navigator) {
          navigator.geolocation.getCurrentPosition(
            async (position) => {
              const { latitude, longitude } = position.coords;
              const response = await fetch(
                `https://api.open-meteo.com/v1/forecast?latitude=${latitude}&longitude=${longitude}&current_weather=true`
              );
              const data = await response.json();
              setTemperature(Math.round(data.current_weather.temperature));
              
              // Simple weather code mapping
              const code = data.current_weather.weathercode;
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
              };
              setWeatherCondition(conditions[code] || "Unknown");
            },
            () => {
              // Default to New York if location denied
              fetchDefaultWeather();
            }
          );
        } else {
          fetchDefaultWeather();
        }
      } catch (e) {
        console.log("Weather fetch failed");
        fetchDefaultWeather();
      }
    };

    const fetchDefaultWeather = async () => {
      try {
        const response = await fetch(
          "https://api.open-meteo.com/v1/forecast?latitude=40.7128&longitude=-74.0060&current_weather=true"
        );
        const data = await response.json();
        setTemperature(Math.round(data.current_weather.temperature));
      } catch (e) {
        setTemperature(72); // Fallback
      }
    };

    fetchWeather();
    // Refresh weather every 30 minutes
    const interval = setInterval(fetchWeather, 30 * 60 * 1000);
    return () => clearInterval(interval);
  }, []);

  // Native Windows Media Controls
  useEffect(() => {
    const fetchMediaInfo = async () => {
      try {
        const info = await invoke<MediaInfo>("get_media_info");

        if (info.has_media) {
          setMediaInfo({
            title: info.title,
            artist: info.artist,
            is_playing: info.is_playing,
            has_media: info.has_media,
            artwork: info.artwork
          });
          setIsPlaying(info.is_playing);
          
          // Try to get album art from Windows media control
          if (info.artwork && info.artwork.length > 0) {
            setAlbumArtUrl(info.artwork[0]);
          }
        } else {
          setIsPlaying(false);
          setMediaInfo({ 
            title: "", 
            artist: "", 
            is_playing: false, 
            has_media: false 
          });
          setAlbumArtUrl(null);
        }
      } catch (e) {
        console.error("Failed to get media info:", e);
        setIsPlaying(false);
      }
    };

    // Initial fetch
    fetchMediaInfo();

    // Poll for media state changes
    const pollInterval = setInterval(fetchMediaInfo, 1000);

    return () => clearInterval(pollInterval);
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

  // Volume monitoring - sync with backend (unused in main window but keeping listener for potential expansion)
  useEffect(() => {
    const unlisten = listen("volume-change", (_event: any) => {
      // Logic for main window volume sync can go here if needed
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


  return (
    <div className="screen">
      <motion.div
        className="bloom"
        initial={{ width: isPlaying ? 200 : 140 }}
        animate={{ width: isHovered ? (isPlaying ? 280 : 280) : (isPlaying ? 200 : 140) }}
        onHoverStart={() => setIsHovered(true)}
        onHoverEnd={() => setIsHovered(false)}
        style={{ originY: 0 }}
        transition={{ type: "spring", stiffness: 400, damping: 25, mass: 0.8 }}
      >
        {/* Left side - Music visualizer OR Passive features */}
        <div className="side-content left">
          <AnimatePresence mode="wait">
            {isHovered ? (
              <motion.div
                key="left-passive"
                className="passive-features-group"
                initial={{ opacity: 0, x: -10 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: -10 }}
                transition={{ duration: 0.12, ease: "easeInOut" }}
              >
                <div
                  className="passive-feature clickable"
                  title={isOnline ? "Connected" : "Offline"}
                  onClick={openWifiSettings}
                >
                  <WifiIcon connected={isOnline} />
                </div>
                <div
                  className="passive-feature clickable"
                  title={isMuted ? "Notifications muted" : "Notifications active"}
                  onClick={openNotificationCenter}
                >
                  {isMuted ? <BellOffIcon /> : <BellIcon />}
                </div>
              </motion.div>
            ) : (
              isPlaying && (
                <motion.div
                  key="visualizer"
                  className="music-visualizer"
                  initial={{ opacity: 0, scale: 0.8 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.8 }}
                  transition={{ duration: 0.12, ease: "easeInOut" }}
                >
                  <Visualizer isPlaying={isPlaying} audioData={audioData} />
                </motion.div>
              )
            )}
          </AnimatePresence>
        </div>

        {/* Center - Time (always visible, stays centered) */}
        <span className="time">{time}</span>

        {/* Right side - Album Art + Passive features */}
        <div className="side-content right">
          {/* Album art - only in compact mode, removed from expanded to fix right-click menu issue */}
          {isPlaying && !isHovered && (
            <motion.button
              key="album-art"
              className="album-art"
              initial={{ opacity: 0, scale: 0.8, x: 10 }}
              animate={{ opacity: 1, scale: 1, x: 0 }}
              exit={{ opacity: 0, scale: 0.8, x: 10 }}
              transition={{ duration: 0.12, ease: "easeInOut", delay: 0.05 }}
              onClick={togglePlayPause}
              title={mediaInfo.title ? `${mediaInfo.title} - ${mediaInfo.artist}` : "Click to pause"}
            >
              {albumArtUrl ? (
                <img
                  src={albumArtUrl}
                  alt="Album art"
                  width="20"
                  height="20"
                  onError={(e) => {
                    (e.target as HTMLImageElement).style.display = "none";
                  }}
                />
              ) : (
                <div className="album-art-placeholder">🎵</div>
              )}
            </motion.button>
          )}

          {/* Passive features - only visible on expand */}
          <AnimatePresence>
            {isHovered && (
              <motion.div
                key="right-passive"
                className="passive-features-group"
                initial={{ opacity: 0, x: 10 }}
                animate={{ opacity: 1, x: 0 }}
                exit={{ opacity: 0, x: 10 }}
                transition={{ duration: 0.08, ease: "easeInOut" }}
              >
                {temperature !== null && (
                  <div className="passive-feature" title={weatherCondition || "Temperature"}>
                    <ThermometerIcon />
                    <span className="label">{temperature}°</span>
                  </div>
                )}
                <div className="passive-feature" title={`${isCharging ? "Charging" : "Battery"} - ${batteryLevel}%`}>
                  <BatteryIcon charging={isCharging} level={batteryLevel} />
                  <span className="label">{batteryLevel}%</span>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </motion.div>
    </div>
  );
}

export default App;
