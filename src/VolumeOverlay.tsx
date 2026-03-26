import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState, useRef } from "react";
import "./VolumeOverlay.css";

// The volume island component that "grows" from and "dissolves" into the left edge
function VolumeNotch({
  volume,
  isMuted,
}: {
  volume: number;
  isMuted: boolean;
}) {
  const percentage = Math.round(volume * 100);

  return (
    <motion.div
      className="volume-notch-wrapper"
      // Initial state: shrunk and transparent with a blur
      initial={{ scale: 0.5, scaleX: 0, opacity: 0, originX: 0, filter: "blur(12px)" }}
      // Animate: organic "pop" out from the edge
      animate={{
        scale: 1,
        scaleX: 1,
        opacity: 1,
        filter: "blur(0px)"
      }}
      // Exit: snap back with high stiffness
      exit={{
        scaleX: 0,
        scale: 0.8,
        opacity: 0,
        filter: "blur(12px)",
        transition: {
          duration: 0.2,
          ease: [0.32, 0.72, 0, 1]
        }
      }}
      transition={{
        type: "spring",
        stiffness: 450, // Higher stiffness for faster response
        damping: 25,    // Lower damping for a juicy, liquid spring
        mass: 0.7       // Lighter mass for snappier movement
      }}
    >
      <div className="volume-notch">
        {/* Staggered content fade-in to prevent distortion during scaling */}
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
          {/* Volume bar track */}
          <div className="volume-notch-bar">
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

          {/* Percentage value at bottom */}
          <div className="volume-notch-text">
            {isMuted ? "0%" : `${percentage}%`}
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}

function VolumeOverlayApp() {
  const [volume, setVolume] = useState(0.5);
  const [isMuted, setIsMuted] = useState(false);
  const [isVisible, setIsVisible] = useState(false);
  const timeoutRef = useRef<any>(null);

  // Listen for volume change events from the system via Tauri
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;

    const setupListener = async () => {
      unlistenFn = await listen("volume-change", (event: any) => {
        const { volume: newVolume, is_muted } = event.payload;

        setVolume(newVolume);
        setIsMuted(is_muted);
        setIsVisible(true);

        // Reset the inactivity timeout whenever volume changes
        if (timeoutRef.current) clearTimeout(timeoutRef.current);

        // Window remains visible for 2 seconds after last change
        timeoutRef.current = setTimeout(() => {
          setIsVisible(false);
        }, 2000);
      });
    };

    setupListener();

    return () => {
      if (unlistenFn) unlistenFn();
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, []);

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

  return (
    <div className="volume-overlay-container">
      <AnimatePresence>
        {isVisible && (
          <VolumeNotch
            volume={volume}
            isMuted={isMuted}
            key="volume-island"
          />
        )}
      </AnimatePresence>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <VolumeOverlayApp />
  </StrictMode>
);
