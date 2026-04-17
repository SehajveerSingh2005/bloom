
import { useState, useEffect } from 'react';
import { motion } from 'framer-motion';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './Dock.css';

// Using simple SVGs for apps that we don't have images for
function TerminalIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="4 17 10 11 4 5"></polyline>
      <line x1="12" y1="19" x2="20" y2="19"></line>
    </svg>
  );
}

function CodeIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <polyline points="16 18 22 12 16 6"></polyline>
      <polyline points="8 6 2 12 8 18"></polyline>
    </svg>
  );
}

function BrowserIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="12" cy="12" r="10"></circle>
      <line x1="2" y1="12" x2="22" y2="12"></line>
      <path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path>
    </svg>
  );
}

function FolderIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"></path>
    </svg>
  );
}

const APPS = [
  { id: 'start', label: 'Start Menu', iconClass: 'bloom-start', icon: '/src-tauri/icons/128x128.png', isImage: true },
  { id: 'explorer', label: 'File Explorer', iconClass: 'explorer', icon: <FolderIcon />, isImage: false },
  { id: 'terminal', label: 'Terminal', iconClass: 'terminal', icon: <TerminalIcon />, isImage: false },
  { id: 'vscode', label: 'VS Code', iconClass: 'vscode', icon: <CodeIcon />, isImage: false },
  { id: 'browser', label: 'Browser', iconClass: 'browser', icon: <BrowserIcon />, isImage: false },
]

export default function Dock() {
  const [dockMode, setDockMode] = useState(() => localStorage.getItem("bloom-dock-mode") || "fixed");
  const [isDockHovered, setIsDockHovered] = useState(false);
  const [isEdgeHovered, setIsEdgeHovered] = useState(false);
  const [isOverlapped, setIsOverlapped] = useState(false);

  useEffect(() => {
    const unlistenSettings = listen<{ key: string, value: any }>("settings-changed", (event) => {
      console.log("[Dock] Settings change received:", event.payload);
      if (event.payload.key === "dock-mode") {
        setDockMode(event.payload.value);
      }
    });

    const unlistenOverlap = listen<boolean>("dock-overlap", (event) => {
      console.log("[Dock] Overlap event received:", event.payload);
      setIsOverlapped(event.payload);
    });

    const handleStorage = (e: StorageEvent) => {
      if (e.key === 'bloom-dock-mode') {
        console.log("[Dock] LocalStorage change detected:", e.newValue);
        setDockMode(e.newValue || 'fixed');
      }
    };
    window.addEventListener('storage', handleStorage);

    return () => {
      unlistenSettings.then(f => f());
      unlistenOverlap.then(f => f());
      window.removeEventListener('storage', handleStorage);
    };
  }, []);

  useEffect(() => {
    const updateRegion = () => {
      const dockElement = document.querySelector('.dock');
      if (dockElement) {
        const rect = dockElement.getBoundingClientRect();
        invoke('update_dock_rect', {
          rect: {
            x: Math.round(rect.x),
            y: Math.round(rect.y),
            width: Math.round(rect.width),
            height: Math.round(rect.height)
          }
        }).catch(err => console.error("[Dock] Failed to update region:", err));
      }
    };

    updateRegion();
    window.addEventListener('resize', updateRegion);
    
    // Also update when dock size might change due to animations or items
    const observer = new ResizeObserver(() => {
        updateRegion();
    });
    
    const dockElement = document.querySelector('.dock');
    if (dockElement) observer.observe(dockElement);
    
    // Periodically update just in case (e.g. during animations)
    const interval = setInterval(updateRegion, 500);

    return () => {
      window.removeEventListener('resize', updateRegion);
      observer.disconnect();
      clearInterval(interval);
    };
  }, []);

  const handleAppClick = async (appId: string) => {
    try {
      if (appId === 'start') {
        await invoke('open_app', { appName: 'start' });
      } else {
        await invoke('open_app', { appName: appId });
      }
    } catch (e) {
      console.error(`Failed to launch ${appId}:`, e);
    }
  };

  const isCurrentlyHovered = isDockHovered || isEdgeHovered;
  const isHidden = dockMode === 'auto-hide' && isOverlapped && !isCurrentlyHovered;

  useEffect(() => {
    invoke('set_dock_hovered', { hovered: isCurrentlyHovered })
      .catch(err => console.error("[Dock] Failed to set hover state:", err));
  }, [isCurrentlyHovered]);

  useEffect(() => {
    // Force region update when visibility changes
    const timeout = setTimeout(() => {
        const updateRegion = () => {
          const dockElement = document.querySelector('.dock');
          if (dockElement) {
            const rect = dockElement.getBoundingClientRect();
            invoke('update_dock_rect', {
              rect: {
                x: Math.round(rect.x),
                y: Math.round(rect.y),
                width: Math.round(rect.width),
                height: Math.round(rect.height)
              }
            }).catch(err => console.error("[Dock] Failed to update region:", err));
          }
        };
        updateRegion();
    }, 100); // Small delay to allow animation to start/complete
    return () => clearTimeout(timeout);
  }, [isHidden]);

  useEffect(() => {
    console.log("[Dock] State Change:", {
      isHidden,
      isOverlapped,
      isCurrentlyHovered,
      dockMode,
      isDockHovered,
      isEdgeHovered
    });
  }, [isHidden, isOverlapped, isCurrentlyHovered, dockMode, isDockHovered, isEdgeHovered]);

  return (
    <div className="dock-container">
      {/* Invisible trigger zone at the very bottom edge of the monitor */}
      <div
        className="activation-zone"
        onMouseEnter={() => {
          console.log("[Dock] Edge hover ENTER");
          setIsEdgeHovered(true);
        }}
        onMouseLeave={() => {
          console.log("[Dock] Edge hover LEAVE");
          setIsEdgeHovered(false);
        }}
      />

      <motion.div
        className="dock"
        onMouseEnter={() => {
          console.log("[Dock] Dock hover ENTER");
          setIsDockHovered(true);
        }}
        onMouseLeave={() => {
          console.log("[Dock] Dock hover LEAVE");
          setIsDockHovered(false);
          setIsEdgeHovered(false);
        }}
        initial={{ y: 0, opacity: 1, scale: 1 }}
        animate={{
          y: isHidden ? 120 : 0,
          opacity: 1,
          scale: 1
        }}
        transition={{ type: "spring", stiffness: 300, damping: 35 }}
      >
        {APPS.map((app) => (
          <div
            key={app.id}
            className="dock-icon-wrapper"
            onClick={() => handleAppClick(app.id)}
          >
            <div className="tooltip">{app.label}</div>
            <motion.div
              className={`dock-icon ${app.iconClass}`}
              transition={{ type: "spring", stiffness: 400, damping: 25, mass: 0.8 }}
            >
              {app.isImage ? <img src={app.icon as string} alt={app.label} /> : app.icon}
            </motion.div>
          </div>
        ))}
      </motion.div>
    </div>
  );
}
