import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback, useRef } from "react";
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
  
  // Audio visualization state
  const [audioData, setAudioData] = useState<number[]>(new Array(5).fill(0.18));

  // Reset visualization when not playing
  useEffect(() => {
    let timeout: any;
    if (isHovered) {
      invoke("set_window_height", { height: 240 });
    } else {
      // Wait for the spring animation to finish before snapping the OS window bounds
      timeout = setTimeout(() => {
        invoke("set_window_height", { height: 40 });
      }, 400);
    }
    return () => clearTimeout(timeout);
  }, [isHovered]);

  // New bloom mode state: 'status' or 'music'
  const [bloomMode, setBloomMode] = useState<'status' | 'music'>('status');
  const [isMuted] = useState(false);

  // Auto-switch to music mode when media is detected
  useEffect(() => {
    if (mediaInfo.has_media && isPlaying) {
      setBloomMode('music');
    }
  }, [mediaInfo.has_media, isPlaying]);

  // Auto-switch back to status mode if music stops for 4 seconds AND not hovered
  useEffect(() => {
    let timer: any;
    if (!isPlaying && bloomMode === 'music' && !isHovered) {
      timer = setTimeout(() => {
        setBloomMode('status');
      }, 4000);
    }
    return () => clearTimeout(timer);
  }, [isPlaying, bloomMode, isHovered]);

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
                224: "Stormy",
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

  // Native Windows Media Controls - Listen for updates from background worker
  useEffect(() => {
    const unlisten = listen<MediaInfo>("media-update", (event) => {
      const info = event.payload;
      // console.log("Received media update:", info);

      if (info && info.has_media) {
        setMediaInfo({
          title: info.title,
          artist: info.artist,
          is_playing: info.is_playing,
          has_media: info.has_media,
          artwork: info.artwork
        });
        setIsPlaying(info.is_playing);
        
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
      // Optimistsic update
      setIsPlaying(prev => !prev);
      await invoke("media_play_pause");
    } catch (e) {
      console.error("Failed to toggle play/pause:", e);
    }
  }, []);

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

  const handleBloomClick = () => {
    if (isPlaying) {
      setBloomMode(prev => prev === 'music' ? 'status' : 'music');
    }
  };

  // Music mode shows any time we have media info (playing or paused)
  const isMusicMode = mediaInfo.has_media && bloomMode === 'music';

  return (
    <div className="screen">
      <motion.div
        className={`bloom ${isHovered ? 'expanded' : ''}`}
        initial={{ width: 140 }}
        animate={{ 
          width: isHovered
            ? (isMusicMode ? 260 : 320)
            : (isMusicMode ? 200 : 140),
          height: isHovered && mediaInfo.has_media && isPlaying ? 64 : 36
        }}
        onClick={handleBloomClick}
        onHoverStart={() => setIsHovered(true)}
        onHoverEnd={() => setIsHovered(false)}
        style={{ originY: 0 }}
        transition={{ type: "spring", stiffness: 400, damping: 25, mass: 0.8 }}
      >
        <div className="main-row">
          {/* Left: visualizer (music) or wifi+notifs (status) */}
          <div className="side-content left">
            <AnimatePresence mode="wait">
              {isMusicMode ? (
                <motion.div
                  key="visualizer"
                  className="music-visualizer"
                  initial={{ opacity: 0, scale: 0.8 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.8 }}
                >
                  <Visualizer isPlaying={isPlaying} audioData={audioData} />
                </motion.div>
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
                  </motion.div>
                )
              )}
            </AnimatePresence>
          </div>

          {/* Center - Time (always visible) */}
          <span className="time">{time}</span>

          {/* Right: album art (music) or battery+temp (status) */}
          <div className="side-content right">
            <AnimatePresence mode="wait">
              {isMusicMode ? (
                <motion.button
                  key="album-art"
                  className={`album-art${isHovered ? ' album-art-large' : ''}${!isPlaying ? ' paused' : ''}`}
                  initial={{ opacity: 0, scale: 0.8 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.8 }}
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
                    {temperature !== null && (
                      <div className="passive-feature" title={weatherCondition}>
                        <ThermometerIcon />
                        <span className="label">{temperature}°</span>
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
        </div>

        {/* Bottom row - Song details (only on hover when playing) */}
        <AnimatePresence>
          {isHovered && isPlaying && mediaInfo.has_media && (
              <motion.div 
                className="bottom-row"
                initial={{ opacity: 0, height: 0, scale: 0.95, filter: "blur(4px)" }}
                animate={{ opacity: 1, height: 28, scale: 1, filter: "blur(0px)" }}
                exit={{ opacity: 0, height: 0, scale: 0.95, filter: "blur(4px)" }}
                transition={{ 
                  default: { type: "spring", stiffness: 400, damping: 25, mass: 0.8 },
                  opacity: { duration: 0.25, ease: "easeOut" },
                  filter: { duration: 0.25, ease: "easeOut" }
                }}
              >
                <MarqueeText title={mediaInfo.title} artist={mediaInfo.artist} />
              </motion.div>
          )}
        </AnimatePresence>
      </motion.div>
    </div>
  );
}

export default App;
