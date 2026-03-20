import { motion, AnimatePresence } from "framer-motion";
import { useEffect, useState, useCallback } from "react";
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
          style={{ scaleY: value }}
          animate={{ opacity: isPlaying ? 0.95 : 0.5 }}
          transition={{ duration: 0.05 }}
        />
      ))}
    </div>
  );
}

interface MediaMetadata {
  title: string;
  artist: string;
  artwork?: { src: string }[];
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
  const [mediaInfo, setMediaInfo] = useState<MediaMetadata>({ title: "", artist: "" });
  const [albumArtUrl, setAlbumArtUrl] = useState<string | null>(null);
  
  // Audio visualization state
  const [audioData, setAudioData] = useState<number[]>(new Array(5).fill(0.3));
  const [useRealAudio, setUseRealAudio] = useState(false);
  const [analyser, setAnalyser] = useState<AnalyserNode | null>(null);
  const [dataArray, setDataArray] = useState<Uint8Array | null>(null);
  
  // Notification state
  const [notificationCount, setNotificationCount] = useState(0);
  const [isMuted, setIsMuted] = useState(false);

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
          });
          setIsPlaying(info.is_playing);
          
          // Try to get album art from Windows media control
          if (info.artwork && info.artwork.length > 0) {
            setAlbumArtUrl(info.artwork[0]);
          }
        } else {
          setIsPlaying(false);
          setMediaInfo({ title: "", artist: "" });
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

  // Audio visualization setup - real system audio capture
  useEffect(() => {
    if (!isPlaying) {
      setAudioData(new Array(5).fill(0.3));
      return;
    }

    // If we already have analyser, just run the visualization loop
    if (analyser && dataArray && useRealAudio) {
      let animationFrameId: number;
      
      const updateVisualizer = () => {
        analyser.getByteFrequencyData(dataArray);
        
        // Get frequency data for 5 bands
        const bands = [0, 1, 2, 3, 4];
        const bandSize = Math.floor(dataArray.length / 10);
        
        const normalized = bands.map(i => {
          // Average across a small frequency range for smoother response
          const start = i * bandSize;
          const end = start + bandSize;
          let sum = 0;
          for (let j = start; j < end; j++) {
            sum += dataArray[j];
          }
          const avg = sum / bandSize / 255;
          
          // Apply response curve
          return Math.pow(avg, 0.7) * 1.1;
        }).map(v => Math.max(0.25, Math.min(0.95, v)));
        
        // Smooth transition
        setAudioData(prev => prev.map((p, i) => p * 0.3 + normalized[i] * 0.7));
        
        animationFrameId = requestAnimationFrame(updateVisualizer);
      };
      
      updateVisualizer();
      
      return () => {
        cancelAnimationFrame(animationFrameId);
      };
    }

    // First time setup - try to capture system audio
    const setupAudioCapture = async () => {
      try {
        const ctx = new AudioContext();
        const analyserNode = ctx.createAnalyser();
        analyserNode.fftSize = 512;
        analyserNode.smoothingTimeConstant = 0.85;

        // Request system audio capture
        const stream = await navigator.mediaDevices.getDisplayMedia({
          video: false,
          audio: true,
        } as MediaStreamConstraints);

        const source = ctx.createMediaStreamSource(stream);
        source.connect(analyserNode);

        const data = new Uint8Array(analyserNode.frequencyBinCount);
        
        setAnalyser(analyserNode);
        setDataArray(data);
        setUseRealAudio(true);

        // Stop video track, keep audio analyser working
        setTimeout(() => {
          stream.getVideoTracks().forEach(t => t.stop());
        }, 100);

      } catch (e) {
        console.log("System audio capture not available");
        setUseRealAudio(false);
        simulateVisualization();
      }
    };

    const simulateVisualization = () => {
      let time = 0;
      let velocities = new Array(5).fill(0);
      let positions = new Array(5).fill(0.3);
      
      const simulate = () => {
        time += 0.08;
        
        setAudioData(prev => {
          const newValues = prev.map((_, i) => {
            // Each bar represents a different frequency band
            const baseFreq = [0.8, 1.2, 1.7, 2.3, 3.1][i];
            const beatFreq = [0.4, 0.4, 0.8, 0.8, 0.4][i];
            
            // Pseudo-random variation per frequency band
            const variation = Math.sin(time * baseFreq + i * 2.1) * Math.cos(time * 0.5 + i * 0.3);
            const beat = Math.sin(time * beatFreq) * Math.cos(time * 0.9) * 0.25;
            
            // Target value
            const target = 0.45 + variation * 0.2 + beat * 0.2;
            
            // Smooth interpolation
            positions[i] += (target - positions[i]) * 0.12;
            
            // Velocity/inertia
            velocities[i] += (positions[i] - prev[i]) * 0.4;
            velocities[i] *= 0.75;
            
            const value = positions[i] + velocities[i] * 0.08;
            
            return Math.max(0.25, Math.min(0.9, value));
          });
          return newValues;
        });
        
        requestAnimationFrame(simulate);
      };
      simulate();
    };

    setupAudioCapture();

    return () => {
      setAudioData(new Array(5).fill(0.3));
    };
  }, [isPlaying, analyser, dataArray, useRealAudio]);

  // Media controls via Tauri commands
  const togglePlayPause = useCallback(async () => {
    try {
      await invoke("media_play_pause");
      setIsPlaying(!isPlaying);
    } catch (e) {
      console.error("Failed to toggle play/pause:", e);
    }
  }, [isPlaying]);

  // Toggle mute/notifications
  const toggleMute = useCallback(() => {
    setIsMuted(!isMuted);
  }, [isMuted]);

  // Demo notification (for testing)
  const triggerNotification = useCallback(() => {
    setNotificationCount((prev) => prev + 1);
  }, []);

  return (
    <div className="screen">
      <motion.div
        className="bloom"
        initial={{ width: isPlaying ? 200 : 140 }}
        animate={{ width: isHovered ? (isPlaying ? 400 : 280) : (isPlaying ? 200 : 140) }}
        onHoverStart={() => setIsHovered(true)}
        onHoverEnd={() => setIsHovered(false)}
        style={{ originY: 0 }}
        transition={{ type: "spring", stiffness: 300, damping: 25 }}
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
                transition={{ duration: 0.15 }}
              >
                <div className="passive-feature" title={isOnline ? "Connected" : "Offline"}>
                  <WifiIcon connected={isOnline} />
                </div>
                <div
                  className="passive-feature clickable"
                  title={isMuted ? "Notifications muted" : "Notifications active"}
                  onClick={toggleMute}
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
                  transition={{ duration: 0.15 }}
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
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              transition={{ duration: 0.15 }}
              onClick={togglePlayPause}
              title="Click to pause"
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
                transition={{ duration: 0.15 }}
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
