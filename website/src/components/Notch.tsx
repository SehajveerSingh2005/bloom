import React, { useState, useEffect, useRef, useCallback, memo } from 'react';
import { motion, AnimatePresence, useAnimation } from 'framer-motion';
import { PlayIcon, PauseIcon, SkipBackIcon, SkipForwardIcon, VolumeLowIcon, VolumeHighIcon } from '../../../src/icons';

interface NotchProps {
  settings: {
    wallpaper: number;
    dockMode: 'fixed' | 'auto-hide';
    notchMode: 'fixed' | 'auto-hide';
    accentColor: string;
    isDockEnabled: boolean;
  };
  playback: {
    isPlaying: boolean;
    trackTitle: string;
    trackArtist: string;
    currentTime: number;
    duration: number;
    volume: number;
    trackIndex?: number;
    trackCover?: string;
    tracksCount?: number;
  };
  setPlaybackState: (state: Partial<NotchProps['playback']>) => void;
  visualizerData: number[];
  onOpenApp: (appId: string) => void;
  updateSetting?: (key: string, value: any) => void;
}

export default function Notch({
  settings,
  playback,
  setPlaybackState,
  visualizerData,
  onOpenApp,
  updateSetting,
}: NotchProps) {
  // Bloom mode state: 'music', 'calendar', 'command-center', or 'status'
  const [bloomMode, setBloomMode] = useState<'music' | 'calendar' | 'command-center' | 'status'>('status');
  const [isHovered, setIsHovered] = useState(false);
  const [isNotchHovered, setIsNotchHovered] = useState(false);
  const [isReady, setIsReady] = useState(false);

  useEffect(() => {
    const timer = setTimeout(() => {
      setIsReady(true);
    }, 240);
    return () => clearTimeout(timer);
  }, []);

  const skipNext = () => {
    const count = playback.tracksCount || 3;
    const nextIdx = ((playback.trackIndex || 0) + 1) % count;
    setPlaybackState({ trackIndex: nextIdx, isPlaying: true, currentTime: 0 });
  };

  const skipPrevious = () => {
    const count = playback.tracksCount || 3;
    const prevIdx = ((playback.trackIndex || 0) - 1 + count) % count;
    setPlaybackState({ trackIndex: prevIdx, isPlaying: true, currentTime: 0 });
  };

  // Track album art changes for flip animation
  const [albumArtKey, setAlbumArtKey] = useState(0);
  const prevTrackCoverRef = useRef(playback.trackCover);
  if (prevTrackCoverRef.current !== playback.trackCover) {
    prevTrackCoverRef.current = playback.trackCover;
    setAlbumArtKey(k => k + 1);
  }

  // Slide-push animation for prev/next buttons
  const prevFront = useAnimation();
  const prevBack = useAnimation();
  const nextFront = useAnimation();
  const nextBack = useAnimation();

  const animatePrev = useCallback(async () => {
    prevFront.set({ x: 0 });
    prevBack.set({ x: 30 });
    prevFront.start({ x: -30, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
    await prevBack.start({ x: 0, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
    skipPrevious();
  }, [skipPrevious, prevFront, prevBack]);

  const animateNext = useCallback(async () => {
    nextFront.set({ x: 0 });
    nextBack.set({ x: -30 });
    nextFront.start({ x: 30, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
    await nextBack.start({ x: 0, transition: { duration: 0.18, ease: [0.4, 0, 0.2, 1] } });
    skipNext();
  }, [skipNext, nextFront, nextBack]);
  
  const [time, setTime] = useState('');
  
  // Custom states matching native app values
  const [wifiEnabled, setWifiEnabled] = useState(true);
  const [bluetoothEnabled, setBluetoothEnabled] = useState(true);
  const [batterySaverEnabled, setBatterySaverEnabled] = useState(false);
  const [currentBrightness, setCurrentBrightness] = useState(70);
  const [dndActive, setDndActive] = useState(false);

  // Read-only mock status values
  const batteryLevel = 85;
  const isCharging = true;
  const temperature = 24;
  const weatherCondition = 'Clear';
  
  // Timer state
  const [timerSeconds, setTimerSeconds] = useState(0);
  const [isTimerRunning, setIsTimerRunning] = useState(false);
  const [isCompactTimerVisible, setIsCompactTimerVisible] = useState(false);
  const [isTimerFinished, setIsTimerFinished] = useState(false);
  const timerIntervalRef = useRef<any>(null);

  // Auto-switch to music mode when playing
  useEffect(() => {
    if (playback.isPlaying && bloomMode !== 'calendar') {
      setBloomMode('music');
    }
  }, [playback.isPlaying, bloomMode]);

  // Auto-switch back from music if music stops for 5 seconds
  useEffect(() => {
    let timer: any;
    if (!playback.isPlaying && bloomMode === 'music') {
      timer = setTimeout(() => {
        setBloomMode('status');
      }, 5000);
    }
    return () => clearTimeout(timer);
  }, [playback.isPlaying, bloomMode]);

  // Update clock time
  useEffect(() => {
    const updateTime = () => {
      const now = new Date();
      setTime(
        now.toLocaleTimeString([], {
          hour: '2-digit',
          minute: '2-digit',
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

  // Timer functions
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
  }, [isTimerRunning, timerSeconds]);

  const formatTimerTime = (totalSeconds: number) => {
    const mins = Math.floor(Math.abs(totalSeconds) / 60);
    const secs = Math.abs(totalSeconds) % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  };

  // Wheel horizontal swipe/scroll gesture to switch modes
  const lastScrollTime = useRef(0);
  const handleWheel = (e: React.WheelEvent) => {
    const target = e.target as HTMLElement;
    if (target.closest('.calendar-grid') || target.closest('.timer-column')) {
      return;
    }

    if (!isHovered) return;

    const now = Date.now();
    if (now - lastScrollTime.current < 250) return;

    const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (Math.abs(delta) < 5) return;

    const modes: ('music' | 'command-center' | 'status' | 'calendar')[] = [
      'music',
      'command-center',
      'status',
      'calendar',
    ];
    const availableModes = modes.filter(m => {
      if (m === 'music' && !playback.trackTitle) return false;
      return true;
    });

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

  const toggleCalendarMode = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isTimerFinished) {
      resetTimer();
      return;
    }

    setBloomMode(prev => {
      if (prev === 'calendar') {
        return playback.isPlaying ? 'music' : 'status';
      }
      return 'calendar';
    });
  };

  const togglePlayPause = useCallback(() => {
    setPlaybackState({ isPlaying: !playback.isPlaying });
  }, [playback.isPlaying, setPlaybackState]);

  const toggleWifi = useCallback(() => {
    setWifiEnabled(prev => !prev);
  }, []);

  const toggleBluetooth = useCallback(() => {
    setBluetoothEnabled(prev => !prev);
  }, []);

  const toggleDockModeSetting = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (updateSetting) {
      const nextMode = settings.dockMode === 'fixed' ? 'auto-hide' : 'fixed';
      updateSetting('dockMode', nextMode);
    }
  }, [settings.dockMode, updateSetting]);

  const toggleNotchModeSetting = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    if (updateSetting) {
      const nextMode = settings.notchMode === 'fixed' ? 'auto-hide' : 'fixed';
      updateSetting('notchMode', nextMode);
    }
  }, [settings.notchMode, updateSetting]);

  const isMusicMode = bloomMode === 'music';
  const isCalendarMode = bloomMode === 'calendar';

  const getDynamicWidth = () => {
    if (isCalendarMode) return 480;
    if (bloomMode === 'command-center' && isHovered) return 350;
    if (bloomMode === 'status' && isHovered) return 280;
    if (isMusicMode && isHovered) return 340;

    let w = 140;
    if (isMusicMode) {
      w = 140;
      if (playback.isPlaying) w += 30;
      w += 30; // album art always enabled in web view

      if (isHovered) {
        w += 60;
      }
    }

    return w;
  };

  const getDynamicHeight = () => {
    if (!isReady) {
      return 36;
    }
    if (bloomMode === 'calendar') return 280;
    if (bloomMode === 'command-center') return isHovered ? 230 : 36;
    if (bloomMode === 'status') return 36;
    if (isMusicMode && isHovered) return 120;
    return 36;
  };

  const isImpacted = true; // Concave corner connector active

  // Hide notch transition class
  const handleNotchModeClass = () => {
    if (settings.notchMode === 'auto-hide' && !isHovered && !isNotchHovered && !isCalendarMode) {
      return 'translate-y-[-100%] opacity-0 pointer-events-none';
    }
    return 'translate-y-0 opacity-100';
  };

  return (
    <div
      className={`absolute top-0 left-0 right-0 h-12 flex justify-center items-start z-[100] transition-all duration-300 pointer-events-none ${handleNotchModeClass()}`}
    >
      <motion.div
        className={`bloom ${isHovered || isNotchHovered || isCalendarMode ? 'expanded' : ''} ${isImpacted ? 'is-impacted' : ''}`}
        onHoverStart={() => {
          setIsHovered(true);
          setIsNotchHovered(true);
        }}
        onHoverEnd={() => {
          setIsHovered(false);
          setIsNotchHovered(false);
          if (bloomMode === 'command-center' || bloomMode === 'calendar' || bloomMode === 'status') {
            setBloomMode(playback.isPlaying ? 'music' : 'status');
          }
        }}
        onWheel={handleWheel}
        onClick={(e) => e.stopPropagation()}
        animate={{
          width: !isReady ? 34 : getDynamicWidth(),
          height: getDynamicHeight(),
          borderBottomLeftRadius: isCalendarMode ? 28 : 18,
          borderBottomRightRadius: isCalendarMode ? 28 : 18,
        }}
        transition={{
          width: { type: 'spring', stiffness: 400, damping: 31 },
          height: { type: 'spring', stiffness: 450, damping: 29 },
          default: { type: 'spring', stiffness: 500, damping: 30, mass: 1 },
        }}
        style={{ originY: 0 }}
      >
        <AnimatePresence mode="wait">
          {isReady && (
            <motion.div
              key="bloom-content"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.1 }}
              style={{
                width: '100%',
                height: '100%',
                display: 'flex',
                flexDirection: 'column',
                alignItems: 'center',
                position: 'relative',
                borderRadius: 'inherit',
              }}
            >
              <AnimatePresence mode="wait">
                {isHovered && isMusicMode && !isCalendarMode ? (
                  /* Music Playing Mode (Hovered) */
                  <motion.div
                    key="expanded-music"
                    className="expanded-music-container"
                    initial={{ opacity: 0, scale: 0.98 }}
                    animate={{ opacity: 1, scale: 1 }}
                    exit={{ opacity: 0, scale: 0.98 }}
                    transition={{ type: 'spring', stiffness: 500, damping: 30 }}
                  >
                    <div className="compact-premium-layout px-4 py-2">
                      <div className="album-art-section">
                        <motion.div
                          className="premium-album-art"
                          whileHover={{ scale: 1.05 }}
                          whileTap={{ scale: 0.95 }}
                        >
                          <AnimatePresence mode="wait" initial={false}>
                            {playback.trackCover ? (
                              <motion.img
                                key={`art-${albumArtKey}`}
                                src={playback.trackCover}
                                alt="Art"
                                className="select-none"
                                initial={{ rotateY: 90, opacity: 0 }}
                                animate={{ rotateY: 0, opacity: 1 }}
                                exit={{ rotateY: -90, opacity: 0 }}
                                transition={{ duration: 0.3, ease: "easeInOut" }}
                                style={{ width: '100%', height: '100%', objectFit: 'cover' }}
                              />
                            ) : (
                              <motion.div
                                key="placeholder"
                                className="w-full h-full bg-white/5 flex items-center justify-center text-3xl select-none"
                                initial={{ rotateY: 90, opacity: 0 }}
                                animate={{ rotateY: 0, opacity: 1 }}
                                exit={{ rotateY: -90, opacity: 0 }}
                                transition={{ duration: 0.3, ease: "easeInOut" }}
                              >
                                🎵
                              </motion.div>
                            )}
                          </AnimatePresence>
                        </motion.div>
                      </div>

                      <div className="metadata-controls-section-middle">
                        <div className="track-header-row">
                          <div className="track-info-middle">
                            <span className="premium-title">{playback.trackTitle}</span>
                            <span className="premium-artist">{playback.trackArtist}</span>
                          </div>
                          <div className="header-visualizer">
                            <Visualizer isPlaying={playback.isPlaying} frequencies={visualizerData} height={18} />
                          </div>
                        </div>

                        <div className="controls-row-sleek">
                          <motion.button
                            className="sleek-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              animatePrev();
                            }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.9 }}
                          >
                            <div style={{ position: 'relative', width: 24, height: 12, overflow: 'hidden' }}>
                              <motion.div animate={prevBack} style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                                <SkipBackIcon size={24} />
                              </motion.div>
                              <motion.div animate={prevFront} style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                                <SkipBackIcon size={24} />
                              </motion.div>
                            </div>
                          </motion.button>

                          <motion.button
                            className="sleek-btn play-pause-btn-floating"
                            onClick={(e) => {
                              e.stopPropagation();
                              togglePlayPause();
                            }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.95 }}
                          >
                            {playback.isPlaying ? <PauseIcon /> : <PlayIcon />}
                          </motion.button>

                          <motion.button
                            className="sleek-btn"
                            onClick={(e) => {
                              e.stopPropagation();
                              animateNext();
                            }}
                            whileHover={{ scale: 1.2 }}
                            whileTap={{ scale: 0.9 }}
                          >
                            <div style={{ position: 'relative', width: 24, height: 12, overflow: 'hidden' }}>
                              <motion.div animate={nextBack} style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                                <SkipForwardIcon size={24} />
                              </motion.div>
                              <motion.div animate={nextFront} style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                                <SkipForwardIcon size={24} />
                              </motion.div>
                            </div>
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
                              value={playback.volume}
                              onChange={(e) => setPlaybackState({ volume: parseFloat(e.target.value) })}
                              onPointerDown={(e) => e.stopPropagation()}
                              onClick={(e) => e.stopPropagation()}
                              className="premium-slider"
                            />
                            <div className="slider-progress-fill" style={{ width: `${playback.volume * 100}%` }} />
                          </div>
                          <VolumeHighIcon />
                        </div>
                      </div>
                    </div>
                  </motion.div>
                ) : (
                  /* Standard Modes (Status / CC / Calendar) */
                  <motion.div
                    key="standard-view-group"
                    className="w-full"
                    initial={{ opacity: 0, y: -5 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: 5 }}
                    transition={{ duration: 0.2 }}
                  >
                    <div className="main-row">
                      <div className="main-row-inner px-2">
                        {/* Left Side: Visualizer or Weather */}
                        <div className="side-content left">
                          {isMusicMode ? (
                            <Visualizer isPlaying={playback.isPlaying} frequencies={visualizerData} />
                          ) : (
                            isHovered && (
                              <motion.div
                                className="passive-features-group"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                              >
                                <div className="passive-feature" title={weatherCondition}>
                                  <ThermometerIcon />
                                  <span className="label">{temperature}°C</span>
                                </div>
                              </motion.div>
                            )
                          )}
                        </div>

                        {/* Center Clock Time */}
                        <div className="time-flip-container" onClick={toggleCalendarMode}>
                          <AnimatePresence initial={false}>
                            {isCompactTimerVisible || isTimerFinished ? (
                              <motion.span
                                key="timer"
                                className={`time ${isTimerFinished ? 'timer-finished' : ''}`}
                                initial={{ rotateX: -90, opacity: 0 }}
                                animate={{ rotateX: 0, opacity: 1 }}
                                exit={{ rotateX: 90, opacity: 0 }}
                                transition={{ type: 'spring', stiffness: 600, damping: 30 }}
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
                                transition={{ type: 'spring', stiffness: 600, damping: 30 }}
                              >
                                {time}
                              </motion.span>
                            )}
                          </AnimatePresence>
                        </div>

                        {/* Right Side: Album Art or Battery */}
                        <div className="side-content right">
                          {isMusicMode ? (
                            <motion.button
                              className={`album-art ${playback.isPlaying ? '' : 'paused'}`}
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
                                <AnimatePresence mode="wait" initial={false}>
                                  {playback.trackCover ? (
                                    <motion.img
                                      key={`compact-art-${albumArtKey}`}
                                      src={playback.trackCover}
                                      alt="Art"
                                      className="object-cover rounded-[3px]"
                                      initial={{ rotateY: 90, opacity: 0 }}
                                      animate={{ rotateY: 0, opacity: 1 }}
                                      exit={{ rotateY: -90, opacity: 0 }}
                                      transition={{ duration: 0.25, ease: "easeInOut" }}
                                      style={{ width: 18, height: 18 }}
                                    />
                                  ) : (
                                    <motion.div
                                      key="compact-placeholder"
                                      className="w-full h-full bg-white/10 flex items-center justify-center text-xs"
                                      initial={{ rotateY: 90, opacity: 0 }}
                                      animate={{ rotateY: 0, opacity: 1 }}
                                      exit={{ rotateY: -90, opacity: 0 }}
                                      transition={{ duration: 0.25, ease: "easeInOut" }}
                                    >
                                      🎵
                                    </motion.div>
                                  )}
                                </AnimatePresence>
                                <div className="album-art-overlay">
                                  <div className="control-icon-small">
                                    {playback.isPlaying ? <PauseIcon /> : <PlayIcon />}
                                  </div>
                                </div>
                              </div>
                            </motion.button>
                          ) : (
                            isHovered && (
                              <motion.div
                                className="passive-features-group"
                                initial={{ opacity: 0 }}
                                animate={{ opacity: 1 }}
                              >
                                <div className="passive-feature">
                                  <BatteryIcon charging={isCharging} level={batteryLevel} />
                                  <span className="label">{batteryLevel}%</span>
                                </div>
                              </motion.div>
                            )
                          )}
                        </div>
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {/* Command Center Panel */}
              <AnimatePresence>
                {bloomMode === 'command-center' && (
                  <motion.div
                    className="absolute top-9 left-0 right-0 px-4 pb-4 flex flex-col gap-3.5"
                    onClick={(e) => e.stopPropagation()}
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0, filter: 'blur(4px)', transition: { duration: 0.1 } }}
                    transition={{ type: 'spring', stiffness: 400, damping: 30 }}
                  >
                    {/* Pills Grid */}
                    <div className="grid grid-cols-2 gap-2 text-xs">
                      {/* Wi-Fi Pill */}
                      <div
                        className={`flex items-center gap-2 p-2 rounded-xl cursor-pointer transition-all ${
                          wifiEnabled
                            ? 'bg-white/15 border border-white/15'
                            : 'bg-white/5 border border-white/5 opacity-55'
                        }`}
                        onClick={toggleWifi}
                      >
                        <WifiIcon connected={wifiEnabled} />
                        <div className="flex flex-col text-left">
                          <span className="font-semibold text-[11px] leading-tight">Wi-Fi</span>
                          <span className="text-[9px] opacity-60 leading-none">
                            {wifiEnabled ? 'Connected' : 'Off'}
                          </span>
                        </div>
                      </div>

                      {/* Dock Mode Pill */}
                      <div
                        className={`flex items-center gap-2 p-2 rounded-xl cursor-pointer transition-all ${
                          settings.dockMode === 'fixed'
                            ? 'bg-white/15 border border-white/15'
                            : 'bg-white/5 border border-white/5 opacity-55'
                        }`}
                        onClick={toggleDockModeSetting}
                      >
                        <DockIcon />
                        <div className="flex flex-col text-left">
                          <span className="font-semibold text-[11px] leading-tight">Dock Mode</span>
                          <span className="text-[9px] opacity-60 leading-none">
                            {settings.dockMode === 'fixed' ? 'Fixed' : 'Auto Hide'}
                          </span>
                        </div>
                      </div>

                      {/* Bluetooth Pill */}
                      <div
                        className={`flex items-center gap-2 p-2 rounded-xl cursor-pointer transition-all ${
                          bluetoothEnabled
                            ? 'bg-white/15 border border-white/15'
                            : 'bg-white/5 border border-white/5 opacity-55'
                        }`}
                        onClick={toggleBluetooth}
                      >
                        <BluetoothIcon />
                        <div className="flex flex-col text-left">
                          <span className="font-semibold text-[11px] leading-tight">Bluetooth</span>
                          <span className="text-[9px] opacity-60 leading-none">
                            {bluetoothEnabled ? 'On' : 'Off'}
                          </span>
                        </div>
                      </div>

                      {/* Notch Mode Pill */}
                      <div
                        className={`flex items-center gap-2 p-2 rounded-xl cursor-pointer transition-all ${
                          settings.notchMode === 'fixed'
                            ? 'bg-white/15 border border-white/15'
                            : 'bg-white/5 border border-white/5 opacity-55'
                        }`}
                        onClick={toggleNotchModeSetting}
                      >
                        <NotchIcon />
                        <div className="flex flex-col text-left">
                          <span className="font-semibold text-[11px] leading-tight">Notch Mode</span>
                          <span className="text-[9px] opacity-60 leading-none">
                            {settings.notchMode === 'fixed' ? 'Fixed' : 'Auto Hide'}
                          </span>
                        </div>
                      </div>
                    </div>

                    {/* Circular Actions Row */}
                    <div className="flex justify-between items-center px-1">
                      <button
                        className={`w-7 h-7 rounded-full flex items-center justify-center border border-white/10 active:scale-95 transition-all ${
                          dndActive ? 'bg-white/20' : 'bg-white/5'
                        }`}
                        onClick={() => setDndActive(prev => !prev)}
                        title="Focus / DND"
                      >
                        <MoonIcon />
                      </button>
                      <button
                        className={`w-7 h-7 rounded-full flex items-center justify-center border border-white/10 active:scale-95 transition-all ${
                          batterySaverEnabled ? 'bg-white/20' : 'bg-white/5'
                        }`}
                        onClick={() => setBatterySaverEnabled(prev => !prev)}
                        title="Energy Saver"
                      >
                        <BatterySaverIcon />
                      </button>
                      <button
                        className="w-7 h-7 rounded-full flex items-center justify-center border border-white/10 bg-white/5 active:scale-95 transition-all"
                        onClick={() => onOpenApp('settings')}
                        title="Bloom Settings"
                      >
                        <SettingsIcon />
                      </button>
                      <button
                        className="w-7 h-7 rounded-full flex items-center justify-center border border-white/10 bg-white/5 active:scale-95 transition-all"
                        onClick={() => onOpenApp('terminal')}
                        title="Terminal Logs"
                      >
                        <ReloadIcon />
                      </button>
                    </div>

                    {/* Sliders Area */}
                    <div className="flex flex-col gap-2 pt-1 border-t border-white/5 text-[11px]">
                      {/* Volume Slider */}
                      <div className="flex items-center gap-2">
                        <VolumeLowIcon />
                        <div className="relative flex-1 h-3 flex items-center bg-white/15 rounded-md overflow-hidden">
                          <input
                            type="range"
                            min="0"
                            max="1"
                            step="0.01"
                            value={playback.volume}
                            onChange={(e) => setPlaybackState({ volume: parseFloat(e.target.value) })}
                            className="absolute inset-0 opacity-0 cursor-pointer z-10"
                          />
                          <div
                            className="h-full bg-white rounded-md"
                            style={{ width: `${playback.volume * 100}%` }}
                          />
                        </div>
                        <span className="w-8 text-right font-medium">{Math.round(playback.volume * 100)}%</span>
                      </div>

                      {/* Brightness Slider */}
                      <div className="flex items-center gap-2">
                        <BrightnessLowIcon />
                        <div className="relative flex-1 h-3 flex items-center bg-white/15 rounded-md overflow-hidden">
                          <input
                            type="range"
                            min="0"
                            max="100"
                            step="1"
                            value={currentBrightness}
                            onChange={(e) => setCurrentBrightness(parseInt(e.target.value))}
                            className="absolute inset-0 opacity-0 cursor-pointer z-10"
                          />
                          <div
                            className="h-full bg-white rounded-md"
                            style={{ width: `${currentBrightness}%` }}
                          />
                        </div>
                        <span className="w-8 text-right font-medium">{currentBrightness}%</span>
                      </div>
                    </div>
                  </motion.div>
                )}
              </AnimatePresence>

              {/* Calendar & Timer Split View */}
              <AnimatePresence>
                {isCalendarMode && (
                  <motion.div
                    className="absolute top-9 left-0 right-0 calendar-timer-content split-view"
                    onClick={(e) => e.stopPropagation()}
                    initial={{ opacity: 0, y: -10 }}
                    animate={{ opacity: 1, y: 0 }}
                    exit={{ opacity: 0, y: -10, filter: 'blur(4px)', transition: { duration: 0.1 } }}
                    transition={{ type: 'spring', stiffness: 400, damping: 30 }}
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
                          <button onClick={resetTimer} className="timer-btn secondary">
                            Reset
                          </button>
                        </div>

                        <div className="timer-presets-new">
                          {[5, 15, 25, 50].map((mins) => (
                            <button
                              key={mins}
                              onClick={() => startTimer(mins)}
                              className="preset-btn-small"
                            >
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

/* Sub-components & SVG Helpers */

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

  for (let i = 0; i < startDay; i++) {
    days.push(<div key={`empty-${i}`} className="calendar-day empty" />);
  }

  const today = new Date().getDate();

  for (let i = 1; i <= totalDays; i++) {
    days.push(
      <div key={i} className={`calendar-day ${i === today ? 'today' : ''}`}>
        {i}
      </div>
    );
  }

  return (
    <div className="calendar-container">
      <div className="calendar-header mb-1 text-[11px] font-semibold text-white/90">
        <span>
          {monthName} {currentYear}
        </span>
      </div>
      <div className="calendar-grid">
        {['S', 'M', 'T', 'W', 'T', 'F', 'S'].map((d, i) => (
          <div key={`${d}-${i}`} className="day-name">
            {d}
          </div>
        ))}
        {days}
      </div>
    </div>
  );
}

function WifiIcon({ connected }: { connected: boolean }) {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={connected ? 'opacity-100' : 'opacity-40'}
    >
      <path d="M5 12.55a11 11 0 0 1 14.08 0" />
      <path d="M1.42 9a16 16 0 0 1 21.16 0" />
      <path d="M8.53 16.11a6 6 0 0 1 6.95 0" />
      <line x1="12" y1="20" x2="12.01" y2="20" />
    </svg>
  );
}

function ThermometerIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M14 4v10.54a4 4 0 1 1-4 0V4a2 2 0 0 1 4 0Z" />
    </svg>
  );
}

function BatteryIcon({ charging, level }: { charging: boolean; level: number }) {
  return (
    <div className="flex items-center relative h-[14px] justify-center">
      <svg width="20" height="10" viewBox="0 0 20 10" fill="none">
        <rect
          x="2"
          y="0.75"
          width="14"
          height="8.5"
          rx="2.4"
          stroke="currentColor"
          strokeOpacity={0.35}
          strokeWidth="1.1"
        />
        <path
          d="M17.5 3.5V6.5"
          stroke="currentColor"
          strokeOpacity={0.35}
          strokeWidth="1.2"
          strokeLinecap="round"
        />
        <rect
          x="3.8"
          y="2.5"
          width={Math.max(0.5, (level / 100) * 10.4)}
          height="5"
          rx="1"
          fill={charging ? '#32D74B' : level <= 20 ? '#FF453A' : 'white'}
        />
      </svg>
      {charging && (
        <div
          style={{
            position: 'absolute',
            top: '50%',
            left: '9px',
            transform: 'translate(-50%, -50%)',
            color: 'white',
          }}
        >
          <svg width="7" height="10" viewBox="0 0 8 12" fill="currentColor">
            <path d="M4.5 0L0 7H3.5L2.5 12L8 5H4.5L5.5 0H4.5Z" />
          </svg>
        </div>
      )}
    </div>
  );
}

const Visualizer = memo(function Visualizer({
  isPlaying,
  frequencies,
  height = 14,
}: {
  isPlaying: boolean;
  frequencies: number[];
  height?: number;
}) {
  return (
    <div className="visualizer-horizontal" style={{ height: `${height}px`, width: `${frequencies.length * 6}px` }}>
      {frequencies.map((value, i) => (
        <motion.div
          key={i}
          className="bar-horizontal"
          animate={{
            scaleY: isPlaying ? Math.max(0.2, value) : 0.1,
            opacity: isPlaying ? 0.95 : 0.5,
          }}
          transition={{
            type: 'spring',
            stiffness: 600,
            damping: 30,
            mass: 0.5,
          }}
        />
      ))}
    </div>
  );
});

function BluetoothIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M6.5 6.5l11 11L12 23V1l5.5 5.5-11 11" />
    </svg>
  );
}

function SettingsIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="opacity-90"
    >
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" />
    </svg>
  );
}

function BrightnessLowIcon() {
  return (
    <svg
      width="12"
      height="12"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className="opacity-50"
    >
      <circle cx="12" cy="12" r="5" fill="currentColor" />
    </svg>
  );
}

function MoonIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M21 12.79A9 9 0 1 1 11.21 3 7 7 0 0 0 21 12.79z" />
    </svg>
  );
}

function DockIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
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
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M4 3h16a2 2 0 0 1 2 2v2a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" />
      <path d="M9 9v4a2 2 0 0 0 2 2h2a2 2 0 0 0 2-2V9" />
    </svg>
  );
}

function ReloadIcon() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <path d="M21.5 2v6h-6M21.34 15.57a10 10 0 1 1-.57-8.38l5.67-5.67" />
    </svg>
  );
}

function BatterySaverIcon() {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2.5"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <rect x="2" y="7" width="16" height="10" rx="2" />
      <path d="M22 11v2" />
      <path d="M6 12h4l2-3v6l-2-3H6" />
    </svg>
  );
}
