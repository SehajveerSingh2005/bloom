import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { motion, AnimatePresence } from "framer-motion";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, useRef, useCallback } from "react";
import "./BrightnessOverlay.css";
import { initTheme } from "./theme";

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
          <div
            ref={barRef}
            className="brightness-notch-bar"
            onMouseDown={handleMouseDown}
            onTouchStart={handleBarInteraction}
            style={{ cursor: 'pointer' }}
          >
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

          <div className="brightness-notch-icon">
            <SunIcon brightness={brightness} />
          </div>
        </motion.div>
      </div>
    </motion.div>
  );
}

function BrightnessOverlayApp() {
  useEffect(() => {
    return initTheme();
  }, []);

  const [brightness, setBrightness] = useState(50);
  const [isVisible, setIsVisible] = useState(false);
  const [brightnessOverlayEnabled, setBrightnessOverlayEnabled] = useState(() => localStorage.getItem("bloom-brightness-overlay-enabled") !== "false");
  const [brightnessEdgeEnabled, setBrightnessEdgeEnabled] = useState(() => localStorage.getItem("bloom-brightness-edge-enabled") !== "false");
  const timeoutRef = useRef<any>(null);
  const [scale, setScale] = useState(() => parseFloat(localStorage.getItem("bloom-scale") || "1.0"));


  useEffect(() => {
    invoke("load_settings").then((settings: any) => {
      if (settings && settings["bloom-scale"] !== undefined) {
        setScale(parseFloat(settings["bloom-scale"]));
      }
    }).catch(console.error);
  }, []);

  useEffect(() => {
    const preventContext = (e: MouseEvent) => e.preventDefault();
    document.addEventListener('contextmenu', preventContext);

    const brightPromise = listen("brightness-change", (event: any) => {
      if (!brightnessOverlayEnabled) return;

      invoke("hide_native_osd");

      const { brightness: newBrightness } = event.payload;

      setBrightness(newBrightness);
      setIsVisible(true);

      if (timeoutRef.current) clearTimeout(timeoutRef.current);

      timeoutRef.current = setTimeout(() => {
        setIsVisible(false);
      }, 2000);
    });

    const edgePromise = listen<boolean>("brightness-edge-hover", (event) => {
      if (!brightnessOverlayEnabled || !brightnessEdgeEnabled) return;

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
      if (event.payload.key === "brightness-overlay") {
        setBrightnessOverlayEnabled(event.payload.value);
        if (!event.payload.value) setIsVisible(false);
      }
      if (event.payload.key === "brightness-edge") {
        setBrightnessEdgeEnabled(event.payload.value);
      }
      if (event.payload.key === "bloom-scale") {
        setScale(Number(event.payload.value));
      }
    });

    return () => {
      brightPromise.then(fn => fn());
      edgePromise.then(fn => fn());
      settingsPromise.then(fn => fn());
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      document.removeEventListener('contextmenu', preventContext);
    };
  }, [brightnessOverlayEnabled, brightnessEdgeEnabled]);

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

  const lastBrightnessCall = useRef(0);

  const handleBrightnessChange = useCallback((newBrightness: number) => {
    setBrightness(newBrightness);

    const now = Date.now();
    if (now - lastBrightnessCall.current < 50) return;
    lastBrightnessCall.current = now;

    invoke("set_brightness", { brightness: Math.round(newBrightness) }).catch(() => {});
  }, []);

  return (
    <div className="brightness-overlay-container">
      <div style={{ zoom: scale, height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'flex-end', width: '100%' }}>
        <AnimatePresence>
          {isVisible && (
            <BrightnessNotch
              brightness={brightness}
              onBrightnessChange={handleBrightnessChange}
              key="brightness-island"
            />
          )}
        </AnimatePresence>
      </div>
    </div>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrightnessOverlayApp />
  </StrictMode>
);
