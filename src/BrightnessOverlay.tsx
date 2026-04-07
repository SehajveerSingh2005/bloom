import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useRef } from "react";
import "./BrightnessOverlay.css";

// The rotating Sun icon component
const SunIcon = ({ brightness }: { brightness: number }) => (
  <motion.div
    className="brightness-icon-container"
    animate={{ rotate: (brightness / 100) * 180 }} // Rotate 180 degrees over the full range
    transition={{ type: "spring", stiffness: 200, damping: 25 }}
  >
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
      <circle cx="12" cy="12" r="5" stroke="currentColor" strokeWidth="2.5" />
      <path d="M12 2V4M12 20V22M4.93 4.93L6.34 6.34M17.66 17.66L19.07 19.07M2 12H4M20 12H22M4.93 19.07L6.34 17.66M17.66 6.34L19.07 4.93" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" />
    </svg>
  </motion.div>
);

// The brightness island component that "grows" from and "dissolves" into the right edge
function BrightnessNotch({
  brightness,
}: {
  brightness: number;
}) {
  return (
    <motion.div
      className="brightness-notch-wrapper"
      style={{ transformOrigin: "right center" }}
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
      <div className="brightness-notch">
        <motion.div
          className="brightness-notch-content-group"
          initial={{ opacity: 0, x: 10 }}
          animate={{ opacity: 1, x: 0 }}
          exit={{ opacity: 0, x: 5, transition: { duration: 0.15 } }}
          transition={{ delay: 0.05, duration: 0.2, ease: "easeOut" }}
        >
          {/* Brightness bar track */}
          <div className="brightness-notch-bar">
            <motion.div
              className="brightness-notch-fill"
              initial={false}
              animate={{ height: `${brightness}%` }}
              transition={{
                type: "spring",
                stiffness: 300,
                damping: 35,
              }}
            />
          </div>

          {/* Rotating Sun Icon at bottom */}
          <div className="brightness-notch-icon">
            <SunIcon brightness={brightness} />
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}

function BrightnessOverlayApp() {
  const [brightness, setBrightness] = useState(50);
  const [isVisible, setIsVisible] = useState(false);
  const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(() => localStorage.getItem("bloom-brightness-overlay-enabled") !== "false");
  const timeoutRef = useRef<any>(null);

  useEffect(() => {
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext);

    let unlistenBright: any = null;
    let unlistenSettings: any = null;

    const setupListeners = async () => {
      unlistenBright = await listen("brightness-change", (event: any) => {
        if (!brightnessOverlayEnabled) return;

        // Backup suppression for native OSD
        invoke("hide_native_osd");

        const { brightness: newBrightness } = event.payload;

        setBrightness(newBrightness);
        setIsVisible(true);

        if (timeoutRef.current) clearTimeout(timeoutRef.current);

        timeoutRef.current = setTimeout(() => {
          setIsVisible(false);
        }, 2000);
      });

      unlistenSettings = await listen<{ key: string, value: boolean }>("settings-changed", (event) => {
        if (event.payload.key === "brightness-overlay") {
          setBrightnessOverlayEnabled(event.payload.value);
          if (!event.payload.value) setIsVisible(false);
        }
      });
    };

    setupListeners();

    return () => {
      if (unlistenBright) unlistenBright();
      if (unlistenSettings) unlistenSettings();
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
    };
  }, [brightnessOverlayEnabled]);

  useEffect(() => {
    let windowHideTimeout: any = null;

    const syncWindow = async () => {
      try {
        const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
        const appWindow = getCurrentWebviewWindow();

        if (isVisible) {
          await appWindow.show();
        } else {
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
    <div className="brightness-overlay-container">
      <AnimatePresence>
        {isVisible && (
          <BrightnessNotch
            brightness={brightness}
            key="brightness-island"
          />
        )}
      </AnimatePresence>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrightnessOverlayApp />
  </StrictMode>
);
