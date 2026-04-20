import { useState, useEffect, useMemo, useRef, memo } from 'react';
import { motion, AnimatePresence, Reorder } from 'framer-motion';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import './Dock.css';

interface AppInfo {
  name: string;
  path: string;
  icon: string | null;
  is_running: boolean;
  is_pinned?: boolean;
  hwnd?: number;
  executable?: string;
}

const Dock = memo(function Dock() {
  const [pinnedApps, setPinnedApps] = useState<AppInfo[]>([]);
  const [activeApps, setActiveApps] = useState<AppInfo[]>([]);
  const iconsRef = useRef<Record<string, string>>({});
  const [, setIconsTick] = useState(0); // For forcing re-renders when icons change
  const [dockMode, setDockMode] = useState(() => localStorage.getItem("bloom-dock-mode") || "fixed");
  const [isDockHovered, setIsDockHovered] = useState(false);
  const [isEdgeHovered, setIsEdgeHovered] = useState(false);
  const [isOverlapped, setIsOverlapped] = useState(false);
  const [showAddPopup, setShowAddPopup] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, app: AppInfo | null } | null>(null);
  const [activeOrder, setActiveOrder] = useState<string[]>([]); // Track order of windows by path
  const [isDragging, setIsDragging] = useState(false);
  const [hoveredApp, setHoveredApp] = useState<string | null>(null);
  
  const dockRef = useRef<HTMLDivElement>(null);

  // Sync geometry to backend for precise interaction tracking
  useEffect(() => {
    const updateRect = () => {
      if (dockRef.current) {
        const rect = dockRef.current.getBoundingClientRect();
        invoke('update_dock_rect', {
          rect: {
            x: Math.round(rect.x),
            y: Math.round(rect.y),
            width: Math.round(rect.width),
            height: Math.round(rect.height)
          }
        }).catch(() => {});
      }
    };

    updateRect();
    window.addEventListener('resize', updateRect);
    const observer = new ResizeObserver(updateRect);
    if (dockRef.current) observer.observe(dockRef.current);

    return () => {
      window.removeEventListener('resize', updateRect);
      observer.disconnect();
    };
  }, [pinnedApps, activeApps]);

  // Load pinned apps on mount
  useEffect(() => {
    const init = async () => {
      const pinned = await invoke<AppInfo[]>('load_pinned_apps');
      setPinnedApps(pinned.map(a => ({ ...a, is_pinned: true })));
      
      // Initial fetch for icons
      pinned.forEach(app => fetchIcon(app.path));
    };
    init();

    const unlistenSettings = listen<{ key: string, value: any }>("settings-changed", (event) => {
      if (event.payload.key === "dock-mode") setDockMode(event.payload.value);
    });

    const unlistenOverlap = listen<boolean>("dock-overlap", (event) => {
      setIsOverlapped(event.payload);
    });

    return () => {
      unlistenSettings.then(f => f());
      unlistenOverlap.then(f => f());
    };
  }, []);

  // Poll for active windows
  useEffect(() => {
    const poll = async () => {
      // Avoid updating apps while dragging to prevent stuttering/freezes
      if (isDragging) return;

      const running = await invoke<AppInfo[]>('get_active_windows');
      setActiveApps(running);
      
      // Maintain stable order
      setActiveOrder(prev => {
        const newPaths = running.map(r => r.path);
        const existingPaths = prev.filter(p => newPaths.includes(p));
        const addedPaths = newPaths.filter(p => !prev.includes(p));
        return [...existingPaths, ...addedPaths];
      });

      // Fetch icons for any new active apps
      running.forEach(app => fetchIcon(app.path, app.hwnd));
    };

    const interval = setInterval(poll, 2000);
    poll();
    return () => clearInterval(interval);
  }, [isDragging]);

  const fetchIcon = async (path: string, hwnd?: number) => {
    const cacheKey = hwnd ? `${path}-${hwnd}` : path;
    if (iconsRef.current[cacheKey]) return;
    
    try {
      const icon = await invoke<string | null>('get_app_icon', { path, hwnd: hwnd || null });
      if (icon) {
        iconsRef.current[cacheKey] = icon;
        iconsRef.current[path] = icon;
        setIconsTick(t => t + 1);
      }
    } catch (e) {
      console.error(`Failed to fetch icon for ${path}:`, e);
    }
  };

  const handleAppClick = async (app: AppInfo) => {
    try {
      if (app.path === 'start') {
        await invoke('open_app', { appName: 'start' });
      } else if (app.hwnd) {
        // Toggle focus/minimize for active windows
        await invoke('focus_window', { hwnd: app.hwnd });
      } else {
        await invoke('open_app', { appName: app.path });
      }
    } catch (e) {
      console.error(`Failed to interact with ${app.name}:`, e);
    }
  };

  const togglePin = async (app: AppInfo) => {
    let newPinned;
    if (app.is_pinned) {
      newPinned = pinnedApps.filter(a => a.path !== app.path);
    } else {
      // Ensure we don't duplicate
      if (pinnedApps.find(a => a.path === app.path)) return;
      newPinned = [...pinnedApps, { ...app, is_pinned: true, is_running: false, hwnd: undefined }];
      fetchIcon(app.path); 
    }
    setPinnedApps(newPinned);
    await invoke('save_pinned_apps', { apps: newPinned });
    closeMenu();
  };

  const menuRef = useRef<HTMLDivElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);

  const handleContextMenu = (e: React.MouseEvent, app: AppInfo | null) => {
    e.stopPropagation();
    e.preventDefault();
    setContextMenu({ x: e.clientX, y: e.clientY, app });
  };

  const closeMenu = () => {
    setContextMenu(null);
    invoke('set_menu_open', { open: false, rect: null }).catch(() => {});
  };

  const closePopup = () => {
    setShowAddPopup(false);
    invoke('set_menu_open', { open: false, rect: null }).catch(() => {});
  };

  // Sync menu/popup state to backend for interactivity
  useEffect(() => {
    let open = false;
    let rect = null;

    if (contextMenu && menuRef.current) {
      const r = menuRef.current.getBoundingClientRect();
      rect = {
        x: Math.round(r.x),
        y: Math.round(r.y),
        width: Math.round(r.width),
        height: Math.round(r.height)
      };
      open = true;
    } else if (showAddPopup) {
      // For the popup, we use full window rect so clicking background works
      rect = {
        x: 0,
        y: 0,
        width: window.innerWidth,
        height: window.innerHeight
      };
      open = true;
    }

    invoke('set_menu_open', { open, rect }).catch(() => {});
  }, [contextMenu, showAddPopup, pinnedApps, activeApps]); // Also re-sync on list changes

  const isCurrentlyHovered = isDockHovered || isEdgeHovered;
  const isHidden = dockMode === 'auto-hide' && isOverlapped && !isCurrentlyHovered;

  // Merge lists: Pinned first, then running but not pinned
  const dockItems = useMemo(() => {
    // Helper to get a stable ID for an app (filename or full path)
    const getAppId = (p: string) => {
      if (!p) return "";
      const normalized = p.toLowerCase().replace(/\\/g, '/');
      const filename = normalized.split('/').pop() || normalized;
      return filename.replace('.lnk', '.exe');
    };

    const runningMap = new Map();
    activeApps.forEach(a => {
      const id = getAppId(a.path);
      if (!runningMap.has(id)) runningMap.set(id, a);
    });
    
    const pinned = [
      { name: 'Start', path: 'start', icon: null, is_running: false, is_pinned: true },
      ...pinnedApps.map(p => {
        const id = getAppId(p.path);
        const running = runningMap.get(id);
        return { ...p, is_running: !!running, hwnd: running?.hwnd };
      })
    ];

    const pinnedIds = new Set(pinned.map(p => getAppId(p.path)));
    const unpinned = activeOrder
      .map(path => runningMap.get(getAppId(path)))
      .filter((a): a is AppInfo => !!a && !pinnedIds.has(getAppId(a.path)));

    return [...pinned, ...unpinned];
  }, [pinnedApps, activeApps, activeOrder]);

  const handleReorder = (newItems: AppInfo[]) => {
    // Ensure 'start' is always at index 0
    let sortedItems = [...newItems];
    const startIndex = sortedItems.findIndex(i => i.path === 'start');
    if (startIndex !== 0 && startIndex !== -1) {
      const start = sortedItems.splice(startIndex, 1)[0];
      sortedItems.unshift(start);
    }

    // Extract paths to check for changes
    const newPinned = sortedItems.filter(item => item.is_pinned && item.path !== 'start');
    const newPinnedPaths = newPinned.map(p => p.path);
    const oldPinnedPaths = pinnedApps.map(p => p.path);

    // Only update if order actually changed
    if (JSON.stringify(newPinnedPaths) !== JSON.stringify(oldPinnedPaths)) {
      setPinnedApps(newPinned);
    }

    const newActivePaths = sortedItems.filter(item => !item.is_pinned).map(item => item.path);
    if (newActivePaths.length > 0) {
      const pinnedPaths = newPinned.map(p => p.path);
      const unpinnedOnly = newActivePaths.filter(p => !pinnedPaths.includes(p));
      
      if (JSON.stringify(unpinnedOnly) !== JSON.stringify(activeOrder)) {
        setActiveOrder(unpinnedOnly);
      }
    }
  };

  const handleDragEnd = () => {
    setIsDragging(false);
    // Save to disk only when dragging finishes
    invoke('save_pinned_apps', { apps: pinnedApps }).catch(console.error);
  };

  useEffect(() => {
    invoke('set_dock_hovered', { hovered: isCurrentlyHovered }).catch(() => {});
  }, [isCurrentlyHovered]);

  const iconVariants = {
    idle: { y: 0, scale: 1 },
    hover: { y: -5, scale: 1.1 },
    drag: { scale: 1.2, y: -10 }
  };


  return (
    <div className={`dock-container ${isDragging ? 'dragging' : ''}`} onClick={closeMenu}>
        <motion.div
          ref={dockRef}
          className="dock"
          onMouseEnter={() => setIsDockHovered(true)}
          onMouseLeave={() => { setIsDockHovered(false); setIsEdgeHovered(false); setHoveredApp(null); }}
          initial={{ y: 100, opacity: 0 }}
          animate={{ y: isHidden ? 100 : 0, opacity: 1 }}
          transition={{ 
            y: { type: "spring", stiffness: 300, damping: 35 },
            opacity: { duration: 0.5 }
          }}
          onContextMenu={(e) => handleContextMenu(e, null)}
        >
          <Reorder.Group
            as="div"
            axis="x"
            values={dockItems}
            onReorder={handleReorder}
            className="dock-reorder-container"
          >
            {dockItems.map((app) => (
              <Reorder.Item
                as="div"
                key={app.path}
                value={app}
                dragListener={!!app.is_pinned && app.path !== 'start'}
                dragMomentum={false}
                dragElastic={0.1}
                layout
                className="dock-icon-wrapper"
                onContextMenu={(e) => handleContextMenu(e, app)}
                onMouseEnter={() => setHoveredApp(app.path)}
                onMouseLeave={() => setHoveredApp(null)}
                onDragStart={() => { if (app.is_pinned && app.path !== 'start') { setIsDragging(true); setHoveredApp(null); } }}
                onDragEnd={handleDragEnd}
              >
                <div className="tooltip">{app.name}</div>
                <motion.div 
                   className="dock-icon"
                   variants={iconVariants}
                   animate={hoveredApp === app.path && !isDragging ? "hover" : "idle"}
                   whileTap={{ scale: 0.9 }}
                   whileDrag={app.is_pinned && app.path !== 'start' ? "drag" : "idle"}
                   onTap={() => {
                     handleAppClick(app);
                   }}
                >
                  {app.path === 'start' ? (
                    <img src="/bloom.png" alt="Bloom" draggable={false} />
                  ) : iconsRef.current[app.path] ? (
                    <img src={iconsRef.current[app.path]} alt={app.name} draggable={false} />
                  ) : (
                    <div className="fallback-icon">{app.name[0]}</div>
                  )}
                </motion.div>
                {app.is_running && <div className="active-indicator" />}
              </Reorder.Item>
            ))}
          </Reorder.Group>
        </motion.div>

      {contextMenu && (
        <div 
          ref={menuRef}
          className="context-menu" 
          style={{ left: contextMenu.x, top: contextMenu.y - (contextMenu.app ? 160 : 60) }}
          onClick={(e) => e.stopPropagation()}
        >
          {contextMenu.app ? (
            <>
              <div className="menu-item" onClick={() => togglePin(contextMenu.app!)}>
                {contextMenu.app.is_pinned ? 'Unpin from Dock' : 'Pin to Dock'}
              </div>
              <div className="menu-divider" />
              <div className="menu-item" onClick={() => { setShowAddPopup(true); closeMenu(); }}>
                Add App to Dock...
              </div>
              <div className="menu-item" onClick={closeMenu}>Options</div>
              {contextMenu.app.is_running && (
                <div className="menu-item quit" onClick={async () => {
                  if (contextMenu.app?.hwnd) {
                    await invoke('close_window', { hwnd: contextMenu.app.hwnd });
                    closeMenu();
                  }
                }}>
                  Quit
                </div>
              )}
            </>
          ) : (
            <div className="menu-item" onClick={() => { setShowAddPopup(true); closeMenu(); }}>
              Add App to Dock...
            </div>
          )}
        </div>
      )}

      <AnimatePresence>
        {showAddPopup && (
          <AddAppPopup 
            containerRef={popupRef}
            onClose={closePopup} 
            onAdd={(app: AppInfo) => { togglePin(app); closePopup(); }}
          />
        )}
      </AnimatePresence>
    </div>
  );
});

function AddAppPopup({ onClose, onAdd, containerRef }: { 
  onClose: () => void, 
  onAdd: (app: AppInfo) => void,
  containerRef: React.RefObject<HTMLDivElement | null>
}) {
  const [apps, setApps] = useState<AppInfo[]>([]);
  const [search, setSearch] = useState('');
  const [debouncedSearch, setDebouncedSearch] = useState('');
  const [loading, setLoading] = useState(true);
  const [listIcons, setListIcons] = useState<Record<string, string>>({});

  // Debounce search input
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 150);
    return () => clearTimeout(timer);
  }, [search]);

  // Handle ESC key and Focus loss
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    const handleBlur = () => onClose();

    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('blur', handleBlur);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('blur', handleBlur);
    };
  }, [onClose]);

  useEffect(() => {
    const load = async () => {
      try {
        const res = await invoke<AppInfo[]>('get_installed_apps');
        // Sort alphabetically
        setApps(res.sort((a, b) => a.name.localeCompare(b.name)));
      } finally {
        setLoading(false);
      }
    };
    load();
  }, []);

  const filtered = useMemo(() => {
    const s = debouncedSearch.toLowerCase();
    if (!s) return apps.slice(0, 15);
    return apps
      .filter(a => a.name.toLowerCase().includes(s))
      .slice(0, 50);
  }, [apps, debouncedSearch]);

  // Batched icon fetching
  useEffect(() => {
    let active = true;
    const fetchVisibleIcons = async () => {
      let batch: Record<string, string> = {};
      let count = 0;

      for (const app of filtered) {
        if (!active) break;
        if (!listIcons[app.path]) {
          // Slightly slower polling to keep system responsive
          await new Promise(r => setTimeout(r, 25)); 
          try {
            const icon = await invoke<string | null>('get_app_icon', { path: app.path });
            if (icon && active) {
              batch[app.path] = icon;
              count++;
              
              // Apply in batches of 4 to balance responsiveness and visual updates
              if (count >= 4) {
                setListIcons(prev => ({ ...prev, ...batch }));
                batch = {};
                count = 0;
              }
            }
          } catch (err) {
            console.error(err);
          }
        }
      }
      
      // Final batch
      if (active && count > 0) {
        setListIcons(prev => ({ ...prev, ...batch }));
      }
    };
    fetchVisibleIcons();
    return () => { active = false; };
  }, [filtered]);

  return (
    <motion.div 
      className="popup-overlay"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
    >
      <motion.div 
        ref={containerRef}
        className="add-app-popup"
        layout
        initial={{ scale: 0.95, opacity: 0, y: 20 }}
        animate={{ scale: 1, opacity: 1, y: 0 }}
        exit={{ scale: 0.95, opacity: 0, y: 20 }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="popup-header">
          <h3>Add to Dock</h3>
          <div className="search-container">
            <input 
              type="text" 
              placeholder="Search applications..." 
              autoFocus 
              value={search}
              onChange={(e) => setSearch(e.target.value)}
            />
          </div>
        </div>
        <div className="apps-list">
          {loading ? (
             <div className="loading-state">
               <div className="spinner"></div>
               <p>Searching for apps...</p>
             </div>
          ) : filtered.length > 0 ? (
            filtered.map(app => (
              <div key={app.path} className="app-list-item" onClick={() => onAdd(app)}>
                <div className="app-list-info">
                  <div className="app-list-icon">
                    {listIcons[app.path] ? (
                      <img src={listIcons[app.path]} alt="" draggable={false} />
                    ) : (
                      <div className="app-icon-placeholder">{app.name[0]}</div>
                    )}
                  </div>
                  <div className="app-name">{app.name}</div>
                </div>
                <div className="app-add-button">Pin</div>
              </div>
            ))
          ) : (
            <div className="no-results">No applications match your search</div>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}

export default Dock;
