import { motion } from "framer-motion";
import { useEffect, useState } from "react";
import { getCurrentWindow, currentMonitor, PhysicalPosition } from "@tauri-apps/api/window";
import "./App.css";

function App() {
  const [time, setTime] = useState("");

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

  return (
    <div className="screen">
      <motion.div
        className="bloom"
        initial={{ width: 140, height: 30 }}
        whileHover={{ width: 200, height: 30 }}
        style={{ originY: 0 }}
        transition={{ type: "spring", stiffness: 200, damping: 20 }}
      >
        <span className="time">{time}</span>
      </motion.div>
    </div>
  );
}

export default App;