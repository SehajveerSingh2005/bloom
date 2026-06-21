import React, { useState, useEffect, useRef, memo } from 'react';
import { motion, AnimatePresence, Reorder } from 'framer-motion';
import { Music, Settings, Terminal } from 'lucide-react';

interface DockItem {
  id: string;
  name: string;
  accent: string;
}

interface DockProps {
  settings: {
    wallpaper: number;
    dockMode: 'fixed' | 'auto-hide';
    notchMode: 'fixed' | 'auto-hide';
    accentColor: string;
    isDockEnabled: boolean;
  };
  openApps: string[];
  minimizedApps: string[];
  onOpenApp: (appId: string) => void;
  onCloseApp: (appId: string) => void;
}

const getAppIcon = (id: string, color: string, size: number = 20) => {
  switch (id) {
    case 'about':
      return <img src="/bloom.png" alt="Bloom Icon" className="object-contain select-none" style={{ width: `${size}px`, height: `${size}px` }} />;
    case 'music':
      return <Music size={size} strokeWidth={2} style={{ color }} />;
    case 'settings':
      return <Settings size={size} strokeWidth={2} style={{ color }} />;
    case 'terminal':
      return <Terminal size={size} strokeWidth={2} style={{ color }} />;
    default:
      return null;
  }
};

const Dock = memo(function Dock({
  settings,
  openApps,
  minimizedApps,
  onOpenApp,
  onCloseApp,
}: DockProps) {
  const [dockItems, setDockItems] = useState<DockItem[]>([
    { id: 'about', name: 'About Bloom', accent: '#e8c5e5' },
    { id: 'music', name: 'Music Player', accent: '#ff2d55' },
    { id: 'settings', name: 'Settings Panel', accent: '#8e8e93' },
    { id: 'terminal', name: 'Developer Logs', accent: '#34c759' },
  ]);

  const [hoveredApp, setHoveredApp] = useState<string | null>(null);
  const [isDockHovered, setIsDockHovered] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; app: DockItem } | null>(null);
  const [isDragging, setIsDragging] = useState(false);
  const [interactionState, setInteractionState] = useState<'active' | 'grace' | 'none'>('none');

  const dockRef = useRef<HTMLDivElement>(null);
  const isAnyInteraction = isDockHovered || !!contextMenu;

  // Interaction grace period
  useEffect(() => {
    if (isAnyInteraction) {
      setInteractionState('active');
    } else if (interactionState !== 'none') {
      setInteractionState('grace');
      const timer = setTimeout(() => setInteractionState('none'), 800);
      return () => clearTimeout(timer);
    }
  }, [isAnyInteraction]);

  const isHidden = settings.dockMode === 'auto-hide' && interactionState === 'none';

  const handleContextMenu = (e: React.MouseEvent, app: DockItem) => {
    e.preventDefault();
    e.stopPropagation();
    setContextMenu({
      x: e.clientX,
      y: e.clientY - 120, // Offset above the dock
      app,
    });
  };

  const closeContextMenu = () => {
    setContextMenu(null);
  };

  useEffect(() => {
    const handleOutsideClick = () => closeContextMenu();
    window.addEventListener('click', handleOutsideClick);
    return () => window.removeEventListener('click', handleOutsideClick);
  }, []);

  const getAppPreviewContent = (id: string) => {
    switch (id) {
      case 'about':
        return 'Bloom Info Card';
      case 'music':
        return 'Now Playing stream';
      case 'settings':
        return 'OS Configurations';
      case 'terminal':
        return 'System terminal logs';
      default:
        return 'Running instance';
    }
  };

  const isAppRunning = (id: string) => openApps.includes(id);

  const iconVariants = {
    idle: { y: 0, scale: 1 },
    hover: { y: -6, scale: 1.15 },
    drag: { y: -10, scale: 1.15, opacity: 0.8 },
    tap: { scale: 0.95 },
  };

  if (!settings.isDockEnabled) return null;

  return (
    <div
      className="absolute bottom-0 left-0 right-0 h-20 flex justify-center items-end z-[90] pb-2 pointer-events-none"
    >
      <motion.div
        ref={dockRef}
        className={`dock ${!isHidden ? 'dock-expanded' : ''} ${isDragging ? 'dragging' : ''}`}
        onMouseEnter={() => setIsDockHovered(true)}
        onMouseLeave={() => {
          setIsDockHovered(false);
          setHoveredApp(null);
        }}
        animate={{
          y: isHidden ? 55 : 0,
          opacity: isHidden ? 0.3 : 1,
        }}
        transition={{ type: 'spring', stiffness: 350, damping: 28 }}
        style={{ pointerEvents: 'auto' }}
      >
        <Reorder.Group
          axis="x"
          values={dockItems}
          onReorder={setDockItems}
          className="dock-reorder-container"
          style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: '12px' }}
        >
          {dockItems.map((app) => {
            const isRunning = isAppRunning(app.id);
            const isMinimized = minimizedApps.includes(app.id);

            return (
              <Reorder.Item
                key={app.id}
                value={app}
                onDragStart={() => setIsDragging(true)}
                onDragEnd={() => setIsDragging(false)}
                className="dock-icon-wrapper"
                style={{ listStyleType: 'none' }}
              >
                {/* Tooltip (Only visible when not dragging and not showing preview) */}
                {!isDragging && hoveredApp === app.id && !isRunning && (
                  <span className="tooltip">{app.name}</span>
                )}

                {/* Window Preview Tooltip */}
                <AnimatePresence>
                  {!isDragging && hoveredApp === app.id && isRunning && !contextMenu && (
                    <motion.div
                      initial={{ opacity: 0, y: 15, scale: 0.95 }}
                      animate={{ opacity: 1, y: 0, scale: 1 }}
                      exit={{ opacity: 0, y: 15, scale: 0.95 }}
                      transition={{ duration: 0.15 }}
                      className="preview-tooltip"
                    >
                      <div className="preview-items">
                        <div
                          className="preview-item"
                          onClick={() => {
                            onOpenApp(app.id);
                            setHoveredApp(null);
                          }}
                        >
                          <div className="text-2xl select-none flex items-center justify-center">
                            {getAppIcon(app.id, app.accent, 24)}
                          </div>
                          <span className="preview-label">{getAppPreviewContent(app.id)}</span>
                        </div>
                      </div>
                    </motion.div>
                  )}
                </AnimatePresence>

                {/* Dock Icon Capsule */}
                <motion.button
                  variants={iconVariants}
                  initial="idle"
                  whileHover={isDragging ? 'drag' : 'hover'}
                  whileTap="tap"
                  onMouseEnter={() => setHoveredApp(app.id)}
                  onMouseLeave={() => setHoveredApp(null)}
                  onContextMenu={(e) => handleContextMenu(e, app)}
                  onClick={() => onOpenApp(app.id)}
                  className="dock-icon"
                  style={{
                    backgroundColor: isRunning ? 'rgba(255,255,255,0.08)' : 'rgba(255,255,255,0.03)',
                    borderColor: isRunning ? 'rgba(255,255,255,0.15)' : 'rgba(255,255,255,0.06)',
                    width: '38px',
                    height: '38px',
                    borderRadius: '12px',
                    fontSize: '20px',
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    cursor: 'pointer',
                  }}
                >
                  {getAppIcon(app.id, app.accent, 20)}
                </motion.button>

                {/* Running App Indicator Dot */}
                {isRunning && (
                  <div
                    className="active-indicator"
                    style={{
                      backgroundColor: isMinimized ? 'rgba(255,255,255,0.3)' : (settings.accentColor || '#ffffff'),
                      boxShadow: isMinimized ? 'none' : `0 0 6px ${settings.accentColor}`,
                    }}
                  />
                )}
              </Reorder.Item>
            );
          })}
        </Reorder.Group>
      </motion.div>

      {/* Custom Context Menu */}
      <AnimatePresence>
        {contextMenu && (
          <motion.div
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            className="absolute bg-[#1a1a24]/95 border border-white/10 rounded-xl p-1.5 w-40 flex flex-col gap-0.5 shadow-2xl select-none z-[100] text-[12px] font-sans text-left"
            style={{ left: contextMenu.x - 80, top: contextMenu.y }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="px-2.5 py-1.5 text-white/30 text-[9px] font-bold uppercase tracking-wider">
              {contextMenu.app.name}
            </div>
            <div className="h-[1px] bg-white/5 my-1" />
            <button
              onClick={() => {
                onOpenApp(contextMenu.app.id);
                closeContextMenu();
              }}
              className="w-full text-left px-2.5 py-1.5 rounded-lg text-white/90 hover:bg-white/5 active:bg-white/10"
            >
              Open Window
            </button>
            {openApps.includes(contextMenu.app.id) && (
              <button
                onClick={() => {
                  onCloseApp(contextMenu.app.id);
                  closeContextMenu();
                }}
                className="w-full text-left px-2.5 py-1.5 rounded-lg text-rose-400 hover:bg-rose-500/10 active:bg-rose-500/15"
              >
                Quit Application
              </button>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
});

export default Dock;
