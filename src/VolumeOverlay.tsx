import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useRef, useCallback } from "react";
import "./VolumeOverlay.css";
import { initTheme } from "./theme";

// The volume island component that "grows" from and "dissolves" into the left edge
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
      animate={{
        scaleX: 1,
        scaleY: 1,
        opacity: 1,
        filter: "blur(0px)",
        y: "-50%"
      }}
      exit={{
        scaleX: 0,
        scaleY: 0.8,
        opacity: 0,
        filter: "blur(12px)",
        y: "-50%",
        transition: {
          duration: 0.2,
          ease: [0.32, 0.72, 0, 1]
        }
      }}
      transition={{
        type: "spring",
        stiffness: 450,
        damping: 25,
        mass: 0.7
      }}
    >
      <div className="volume-notch">
        <motion.div
          className="volume-notch-content-group"
          initial={{ opacity: 0, x: -10 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{
            opacity: 0,
            x: -5,
            transition: { duration: 0.15 }
          }}
          transition={{
            delay: 0.05,
            duration: 0.2,
            ease: "easeOut"
          }}
        >
          <div
            ref={barRef}
            className="volume-notch-bar"
            onMouseDown={handleMouseDown}
            onTouchStart={handleBarInteraction}
            style={{ cursor: 'pointer' }}
          >
            <motion.div
              className="volume-notch-fill"
              initial={false}
              animate={{ height: isMuted ? "0%" : `${percentage}%` }}
              transition={{
                type: "spring",
                stiffness: 300,
                damping: 35,
              }}
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

function VolumeOverlayApp() {
  useEffect(() => {
    return initTheme();
  }, []);

  const [volume, setVolume] = useState(0.5);
  const [isMuted, setIsMuted] = useState(false);
  const [isVisible, setIsVisible] = useState(false);
  const [volumeOverlayEnabled, setVolumeOverlayEnabled] = useState(() => localStorage.getItem("bloom-volume-overlay-enabled") !== "false");
  const [volumeEdgeEnabled, setVolumeEdgeEnabled] = useState(() => localStorage.getItem("bloom-volume-edge-enabled") !== "false");
  const timeoutRef = useRef<any>(null);
  const [scale, setScale] = useState(() => parseFloat(localStorage.getItem("bloom-scale") || "1.0"));


  useEffect(() => {
    invoke("load_settings").then((settings: any) => {
      if (settings && settings["bloom-scale"] !== undefined) {
        setScale(parseFloat(settings["bloom-scale"]));
      }
    }).catch(console.error);
  }, []);

  // Listen for volume change events and settings changes
  useEffect(() => {
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext);

    const volPromise = listen("volume-change", (event: any) => {
      if (!volumeOverlayEnabled) return;

      invoke("hide_native_osd");

      const { volume: newVolume, is_muted } = event.payload;

      setVolume(newVolume);
      setIsMuted(is_muted);
      setIsVisible(true);

      if (timeoutRef.current) clearTimeout(timeoutRef.current);

      timeoutRef.current = setTimeout(() => {
        setIsVisible(false);
      }, 2000);
    });

    const edgePromise = listen<boolean>("volume-edge-hover", (event) => {
      if (!volumeOverlayEnabled || !volumeEdgeEnabled) return;

      if (event.payload) {
        setIsVisible(true);
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
      } else {
        if (timeoutRef.current) clearTimeout(timeoutRef.current);
        timeoutRef.current = setTimeout(() => {
          setIsVisible(false);
        }, 1500);
      }
    });

    const settingsPromise = listen<{ key: string, value: any }>("settings-changed", (event) => {
      if (event.payload.key === "volume-overlay") {
        setVolumeOverlayEnabled(event.payload.value);
        if (!event.payload.value) setIsVisible(false);
      }
      if (event.payload.key === "volume-edge") {
        setVolumeEdgeEnabled(event.payload.value);
      }
      if (event.payload.key === "bloom-scale") {
        setScale(Number(event.payload.value));
      }
    });

    return () => {
      volPromise.then(fn => fn());
      edgePromise.then(fn => fn());
      settingsPromise.then(fn => fn());
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      document.removeEventListener('contextmenu', preventContext);
    };
  }, [volumeOverlayEnabled, volumeEdgeEnabled]);

  // Manage window visibility to allow exit animation to finish before hiding the window
  useEffect(() => {
    let windowHideTimeout: any = null;

    const syncWindow = async () => {
      try {
        const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        const appWindow = getCurrentWebviewWindow();

        if (isVisible) {
          await appWindow.show();
        } else {
          // Wait long enough for the exit animation (0.25s) to finish
          windowHideTimeout = setTimeout(async () => {
            await appWindow.hide();
          }, 400);
        }
      } catch (e) {
        console.error("Window management error:", e);
      }
    };

    syncWindow();

    return () => {
      if (windowHideTimeout) clearTimeout(windowHideTimeout);
    };
  }, [isVisible]);

  const lastVolumeCall = useRef(0);

  const handleVolumeChange = useCallback((newVol: number) => {
    setVolume(newVol);
    setIsMuted(newVol === 0);

    const now = Date.now();
    if (now - lastVolumeCall.current < 50) return;
    lastVolumeCall.current = now;

    invoke("set_volume", { volume: newVol }).catch(() => {});
  }, []);

  return (
    <div className="volume-overlay-container">
      <div style={{ zoom: scale, height: '100%', display: 'flex', alignItems: 'center' }}>
        <AnimatePresence>
          {isVisible && (
            <VolumeNotch
              volume={volume}
              isMuted={isMuted}
              onVolumeChange={handleVolumeChange}
              key="volume-island"
            />
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <VolumeOverlayApp />
  </StrictMode>
);
