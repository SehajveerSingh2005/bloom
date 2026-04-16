
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
  const [isHovered, setIsHovered] = useState(false);
  const [isOverlapped, setIsOverlapped] = useState(false);

  useEffect(() => {
    const unlistenSettings = listen<{ key: string, value: any }>("settings-changed", (event) => {
      if (event.payload.key === "dock-mode") {
        setDockMode(event.payload.value);
      }
    });

    const unlistenOverlap = listen<boolean>("dock-overlap", (event) => {
      setIsOverlapped(event.payload);
    });

    return () => { 
      unlistenSettings.then(f => f());
      unlistenOverlap.then(f => f());
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

  // Logic: 
  // - Auto-hide: Hidden if (Overlapped AND NOT Hovered)
  let isHidden = false;
  if (dockMode === 'auto-hide') {
    if (isOverlapped && !isHovered) {
      isHidden = true;
    }
  }

  return (
    <div 
      className="dock-container" 
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      <motion.div 
        className="dock"
        initial={{ y: 100, opacity: 0 }}
        animate={{ 
          y: isHidden ? 80 : 0, 
          opacity: 1
        }}
        transition={{ type: "spring", stiffness: 300, damping: 30 }}
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
