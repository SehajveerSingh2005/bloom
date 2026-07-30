import { StrictMode, useState, useEffect, useRef, useCallback } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen, emit } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import "./Overlay.css";
import { initTheme } from "./theme";

// ─── Volume Notch ───────────────────────────────────────────────────────────

function VolumeNotch({
  volume,
  isMuted,
  onVolumeChange,
}: {
  volume: number;
  isMuted: boolean;
  onVolumeChange: (vol: number) => void;
}) {
  const percentage = Math.round(volume * 100);
  const barRef = useRef<HTMLDivElement>(null);

  const handleBarInteraction = (e: React.MouseEvent | React.TouchEvent) => {
    if (!barRef.current) return;
    const rect = barRef.current.getBoundingClientRect();
    const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY;
    const relativeY = rect.bottom - clientY;
    const newVolume = Math.max(0, Math.min(1, relativeY / rect.height));
    onVolumeChange(newVolume);
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    handleBarInteraction(e);
    const handleMouseMove = (moveE: MouseEvent) => {
      if (!barRef.current) return;
      const rect = barRef.current.getBoundingClientRect();
      const relativeY = rect.bottom - moveE.clientY;
      const newVolume = Math.max(0, Math.min(1, relativeY / rect.height));
      onVolumeChange(newVolume);
    };
    const handleMouseUp = () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  };

  return (
    <motion.div
      className="volume-notch-wrapper"
      style={{ transformOrigin: "left center" }}
      initial={{ scaleX: 0, scaleY: 0.5, opacity: 0, filter: "blur(12px)", y: "-50%" }}
      animate={{ scaleX: 1, scaleY: 1, opacity: 1, filter: "blur(0px)", y: "-50%" }}
      exit={{ scaleX: 0, scaleY: 0.8, opacity: 0, filter: "blur(12px)", y: "-50%", transition: { duration: 0.2, ease: [0.32, 0.72, 0, 1] } }}
      transition={{ type: "spring", stiffness: 450, damping: 25, mass: 0.7 }}
    >
      <div className="volume-notch">
        <motion.div
          className="volume-notch-content-group"
          initial={{ opacity: 0, x: -10 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: -5, transition: { duration: 0.15 } }}
          transition={{ delay: 0.05, duration: 0.2, ease: "easeOut" }}
        >
          <div ref={barRef} className="volume-notch-bar" onMouseDown={handleMouseDown} onTouchStart={handleBarInteraction} style={{ cursor: 'pointer' }}>
            <motion.div
              className="volume-notch-fill"
              initial={false}
              animate={{ height: isMuted ? "0%" : `${percentage}%` }}
              transition={{ type: "spring", stiffness: 300, damping: 35 }}
            />
          </div>
          <div className="volume-notch-text">
            {isMuted ? "0%" : `${percentage}%`}
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}

// ─── Brightness Notch ───────────────────────────────────────────────────────

const SunIcon = ({ brightness }: { brightness: number }) => (
  <motion.div
    className="brightness-icon-container"
    animate={{ rotate: (brightness / 100) * 180 }}
    transition={{ type: "spring", stiffness: 200, damping: 25 }}
  >
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="5" stroke="currentColor" strokeWidth="2.5" />
      <path d="M12 2V4M12 20V22M4.93 4.93L6.34 6.34M17.66 17.66L19.07 19.07M2 12H4M20 12H22M4.93 19.07L6.34 17.66M17.66 6.34L19.07 4.93" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  </motion.div>
);

function BrightnessNotch({
  brightness,
  onBrightnessChange,
}: {
  brightness: number;
  onBrightnessChange: (val: number) => void;
}) {
  const barRef = useRef<HTMLDivElement>(null);

  const handleBarInteraction = (e: React.MouseEvent | React.TouchEvent) => {
    if (!barRef.current) return;
    const rect = barRef.current.getBoundingClientRect();
    const clientY = 'touches' in e ? e.touches[0].clientY : e.clientY;
    const relativeY = rect.bottom - clientY;
    const newBrightness = Math.max(0, Math.min(100, (relativeY / rect.height) * 100));
    onBrightnessChange(newBrightness);
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    handleBarInteraction(e);
    const handleMouseMove = (moveE: MouseEvent) => {
      if (!barRef.current) return;
      const rect = barRef.current.getBoundingClientRect();
      const relativeY = rect.bottom - moveE.clientY;
      const newBrightness = Math.max(0, Math.min(100, (relativeY / rect.height) * 100));
      onBrightnessChange(newBrightness);
    };
    const handleMouseUp = () => {
      document.removeEventListener('mousemove', handleMouseMove);
      document.removeEventListener('mouseup', handleMouseUp);
    };
    document.addEventListener('mousemove', handleMouseMove);
    document.addEventListener('mouseup', handleMouseUp);
  };

  return (
    <motion.div
      className="brightness-notch-wrapper"
      style={{ transformOrigin: "right center" }}
      initial={{ scaleX: 0, scaleY: 0.5, opacity: 0, filter: "blur(12px)", y: "-50%" }}
      animate={{ scaleX: 1, scaleY: 1, opacity: 1, filter: "blur(0px)", y: "-50%" }}
      exit={{ scaleX: 0, scaleY: 0.8, opacity: 0, filter: "blur(12px)", y: "-50%", transition: { duration: 0.2, ease: [0.32, 0.72, 0, 1] } }}
      transition={{ type: "spring", stiffness: 450, damping: 25, mass: 0.7 }}
    >
      <div className="brightness-notch">
        <motion.div
          className="brightness-notch-content-group"
          initial={{ opacity: 0, x: 10 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 5, transition: { duration: 0.15 } }}
          transition={{ delay: 0.05, duration: 0.2, ease: "easeOut" }}
        >
          <div ref={barRef} className="brightness-notch-bar" onMouseDown={handleMouseDown} onTouchStart={handleBarInteraction} style={{ cursor: 'pointer' }}>
            <motion.div
              className="brightness-notch-fill"
              initial={false}
              animate={{ height: `${brightness}%` }}
              transition={{ type: "spring", stiffness: 300, damping: 35 }}
            />
          </div>
          <div className="brightness-notch-icon">
            <SunIcon brightness={brightness} />
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}

// ─── Main Overlay Component ─────────────────────────────────────────────────

function OverlayApp() {
  useEffect(() => {
    return initTheme();
  }, []);

  type Mode = 'idle' | 'volume' | 'brightness' | 'splash' | 'updating';
  const [mode, setMode] = useState<Mode>('idle');
  const [updateStatus, setUpdateStatus] = useState<string>('checking');
  const [updateProgress, setUpdateProgress] = useState(0);

  // Volume state
  const [volume, setVolume] = useState(0.5);
  const [isMuted, setIsMuted] = useState(false);
  const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(() => localStorage.getItem("bloom-volume-overlay-enabled") !== "false");
  const [volumeEdgeEnabled, setVolumeEdgeEnabled] = useState(() => localStorage.getItem("bloom-volume-edge-enabled") !== "false");

  // Brightness state
  const [brightness, setBrightness] = useState(50);
  const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(() => localStorage.getItem("bloom-brightness-overlay-enabled") !== "false");
  const [brightnessEdgeEnabled, setBrightnessEdgeEnabled] = useState(() => localStorage.getItem("bloom-brightness-edge-enabled") !== "false");

  // Shared state
  const [scale, setScale] = useState(() => parseFloat(localStorage.getItem("bloom-scale") || "1.0"));
  const timeoutRef = useRef<any>(null);
  const hideWindowTimeoutRef = useRef<any>(null);
  const splashActiveRef = useRef(false);

  const resetHideTimeout = useCallback(() => {
    if (timeoutRef.current) clearTimeout(timeoutRef.current);
    timeoutRef.current = setTimeout(() => {
      if (!splashActiveRef.current) setMode('idle');
    }, 2000);
  }, []);

  // Load scale from settings
  useEffect(() => {
    invoke("load_settings").then((settings: any) => {
      if (settings && settings["bloom-scale"] !== undefined) {
        setScale(parseFloat(settings["bloom-scale"]));
      }
    }).catch(console.error);
  }, []);

  // ── Splash Detection ──
  useEffect(() => {
    const firstRun = localStorage.getItem("bloom-first-run") === null;
    const storedVersion = localStorage.getItem("bloom-app-version");

    const showSplash = (version?: string) => {
      splashActiveRef.current = true;
      setMode('splash');
      invoke('set_splash_fullscreen', { fullscreen: true });
      if (version) localStorage.setItem("bloom-app-version", version);
      setTimeout(() => emit('splash-done'), 2800);
    };

    // First run or old version without version key — splash immediately
    if (firstRun || storedVersion === null) {
      showSplash();
      // Still try to store the version in the background
      getVersion().then((v) => localStorage.setItem("bloom-app-version", v)).catch(() => {});
      return;
    }

    // Has version key — check if it matches
    getVersion().then((currentVersion) => {
      if (storedVersion !== currentVersion) {
        showSplash(currentVersion);
      } else {
        emit('splash-done');
      }
    }).catch(() => {
      // Version check failed — let bloom start
      emit('splash-done');
    });
  }, []);

  const onSplashComplete = useCallback(() => {
    localStorage.setItem("bloom-first-run", "done");
    setTimeout(() => {
      splashActiveRef.current = false;
      setMode('idle');
      invoke('set_splash_fullscreen', { fullscreen: false });
    }, 300);
  }, []);

  // ── Event Listeners ──
  useEffect(() => {
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext);

    const volPromise = listen("volume-change", (event: any) => {
      if (splashActiveRef.current || !volumeOverlayEnabled) return;
      invoke("hide_native_osd");
      const { volume: newVolume, is_muted } = event.payload;
      setVolume(newVolume);
      setIsMuted(is_muted);
      setMode('volume');
      resetHideTimeout();
    });

    const volEdgePromise = listen<boolean>("volume-edge-hover", (event) => {
      if (splashActiveRef.current || !volumeOverlayEnabled || !volumeEdgeEnabled) return;
      if (event.payload) {
        setMode('volume');
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
      } else {
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
        timeoutRef.current = setTimeout(() => setMode('idle'), 1500);
      }
    });

    const brightPromise = listen("brightness-change", (event: any) => {
      if (splashActiveRef.current || !brightnessOverlayEnabled) return;
      invoke("hide_native_osd");
      const { brightness: newBrightness } = event.payload;
      setBrightness(newBrightness);
      setMode('brightness');
      resetHideTimeout();
    });

    const brightEdgePromise = listen<boolean>("brightness-edge-hover", (event) => {
      if (splashActiveRef.current || !brightnessOverlayEnabled || !brightnessEdgeEnabled) return;
      if (event.payload) {
        setMode('brightness');
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
      } else {
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
        timeoutRef.current = setTimeout(() => setMode('idle'), 1500);
      }
    });

    const settingsPromise = listen<{ key: string, value: any }>("settings-changed", (event) => {
      if (splashActiveRef.current) return;
      const { key, value } = event.payload;
      if (key === "volume-overlay") {
        setVolumeOverlayEnabled(value);
        if (!value && mode === 'volume') setMode('idle');
      }
      if (key === "volume-edge") setVolumeEdgeEnabled(value);
      if (key === "brightness-overlay") {
        setBrightnessOverlayEnabled(value);
        if (!value && mode === 'brightness') setMode('idle');
      }
      if (key === "brightness-edge") setBrightnessEdgeEnabled(value);
      if (key === "bloom-scale") setScale(Number(value));
    });

    const autoUpdatePromise = listen<{ status: string; progress?: number }>("auto-update-status", (event) => {
      const { status, progress } = event.payload;
      setUpdateStatus(status);
      if (progress !== undefined) setUpdateProgress(progress);

      if (status === 'checking' || status === 'downloading' || status === 'installing') {
        splashActiveRef.current = true;
        setMode('updating');
        invoke('set_splash_fullscreen', { fullscreen: true });
      } else if (status === 'done') {
        splashActiveRef.current = false;
        setMode('idle');
        invoke('set_splash_fullscreen', { fullscreen: false });
      }
    });

    return () => {
      volPromise.then(fn => fn());
      volEdgePromise.then(fn => fn());
      brightPromise.then(fn => fn());
      brightEdgePromise.then(fn => fn());
      settingsPromise.then(fn => fn());
      autoUpdatePromise.then(fn => fn());
      document.removeEventListener('contextmenu', preventContext);
    };
  }, [volumeOverlayEnabled, volumeEdgeEnabled, brightnessOverlayEnabled, brightnessEdgeEnabled, resetHideTimeout]);

  // ── Window Visibility Management ──
  useEffect(() => {
    const syncWindow = async () => {
      try {
        const appWindow = getCurrentWebviewWindow();
        if (hideWindowTimeoutRef.current) {
          clearTimeout(hideWindowTimeoutRef.current);
          hideWindowTimeoutRef.current = null;
        }

        if (mode === 'idle') {
          // Wait for exit animation to finish before hiding
          hideWindowTimeoutRef.current = setTimeout(async () => {
            await appWindow.hide();
          }, 400);
        } else {
          // Position the window first, then show
          await invoke('sync_overlay_position');
          await appWindow.show();
        }
      } catch (e) {
        console.error("Window management error:", e);
      }
    };

    // Don't manage window visibility during splash — Rust handles it
    if (mode !== 'splash' && !splashActiveRef.current) {
      syncWindow();
    }

    return () => {
      if (hideWindowTimeoutRef.current) clearTimeout(hideWindowTimeoutRef.current);
    };
  }, [mode]);

  // ── Volume Controls ──
  const lastVolumeCall = useRef(0);
  const handleVolumeChange = useCallback((newVol: number) => {
    setVolume(newVol);
    setIsMuted(newVol === 0);
    setMode('volume');
    resetHideTimeout();

    const now = Date.now();
    if (now - lastVolumeCall.current < 50) return;
    lastVolumeCall.current = now;
    invoke("set_volume", { volume: newVol }).catch(() => {});
  }, [resetHideTimeout]);

  // ── Brightness Controls ──
  const lastBrightnessCall = useRef(0);
  const handleBrightnessChange = useCallback((newBrightness: number) => {
    setBrightness(newBrightness);
    setMode('brightness');
    resetHideTimeout();

    const now = Date.now();
    if (now - lastBrightnessCall.current < 50) return;
    lastBrightnessCall.current = now;
    invoke("set_brightness", { brightness: Math.round(newBrightness) }).catch(() => {});
  }, [resetHideTimeout]);

  const isLeft = mode === 'volume';

  return (
    <div className="overlay-container">
      {/* Splash Screen */}
      <AnimatePresence>
        {mode === 'splash' && (
          <motion.div
            className="splash-screen"
            initial={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.3 }}
          >
            <motion.img
              src="/bloom.png"
              className="splash-logo"
              initial={{ scale: 0, opacity: 0, rotate: 0 }}
              animate={{
                scale: [0, 1.1, 1, 1.2, 1, 1, 0.2],
                opacity: [0, 1, 1, 1, 1, 1, 0],
                rotate: [0, 0, 0, 0, 0, 540, 1080],
              }}
              transition={{
                duration: 3.2,
                times: [0, 0.17, 0.3, 0.43, 0.56, 0.78, 1],
                ease: ["easeOut", "easeInOut", "easeInOut", "easeInOut", "linear", "linear"],
              }}
              onAnimationComplete={onSplashComplete}
            />
          </motion.div>
        )}
      </AnimatePresence>

      {/* Auto-Update Splash */}
      <AnimatePresence>
        {mode === 'updating' && (
          <motion.div
            className="update-splash"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.3 }}
          >
            <img src="/bloom.png" className="update-splash-logo" alt="Bloom" />
            <p className="update-splash-text">
              {updateStatus === 'checking' && "Checking for updates..."}
              {updateStatus === 'downloading' && `Downloading update... ${updateProgress}%`}
              {updateStatus === 'installing' && "Installing update..."}
            </p>
            {updateStatus === 'downloading' && (
              <div className="update-progress-bar">
                <div
                  className="update-progress-fill"
                  style={{ width: `${updateProgress}%` }}
                />
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* Volume / Brightness Overlay */}
      {mode !== 'splash' && mode !== 'updating' && (
        <div style={{ zoom: scale, height: '100%', display: 'flex', alignItems: 'center', justifyContent: isLeft ? 'flex-start' : 'flex-end', width: '100%' }}>
          <AnimatePresence mode="wait">
            {mode === 'volume' && (
              <VolumeNotch
                volume={volume}
                isMuted={isMuted}
                onVolumeChange={handleVolumeChange}
                key="volume-island"
              />
            )}
            {mode === 'brightness' && (
              <BrightnessNotch
                brightness={brightness}
                onBrightnessChange={handleBrightnessChange}
                key="brightness-island"
              />
            )}
          </AnimatePresence>
        </div>
      )}
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <OverlayApp />
  </StrictMode>
);
