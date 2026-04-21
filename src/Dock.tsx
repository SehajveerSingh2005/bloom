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
  const [, setIconsTick] = useState(0); 
  const [dockMode, setDockMode] = useState(() => localStorage.getItem("bloom-dock-mode") || "fixed");
  const [isDockHovered, setIsDockHovered] = useState(false);
  const [isEdgeHovered, setIsEdgeHovered] = useState(false);
  const [isOverlapped, setIsOverlapped] = useState(false);
  const [showAddPopup, setShowAddPopup] = useState(false);
  const [contextMenu, setContextMenu] = useState<{ x: number, y: number, app: AppInfo | null } | null>(null);
  const [activeSubmenu, setActiveSubmenu] = useState<string | null>(null);
  const [activeOrder, setActiveOrder] = useState<string[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [hoveredApp, setHoveredApp] = useState<string | null>(null);
  const [pressedApp, setPressedApp] = useState<string | null>(null);
  const [isReady, setIsReady] = useState(false);
  const [isImpacted, setIsImpacted] = useState(false);
  const [isExpanded, setIsExpanded] = useState(false);
  const dockRef = useRef<HTMLDivElement>(null);

  const isCurrentlyHovered = isDockHovered || isEdgeHovered;
  const [interactionState, setInteractionState] = useState<'active' | 'grace' | 'none'>('none');
  const isAnyInteraction = isCurrentlyHovered || !!contextMenu || showAddPopup;

  useEffect(() => {
    if (isAnyInteraction) {
      setInteractionState('active');
    } else if (interactionState !== 'none') {
      setInteractionState('grace');
      const timer = setTimeout(() => setInteractionState('none'), 2000);
      return () => clearTimeout(timer);
    }
  }, [isAnyInteraction]);

  const isHidden = dockMode === 'auto-hide' && isOverlapped && interactionState === 'none';

  useEffect(() => {
    // Poll for visibility to trigger entrance animation
    const checkVisibility = async () => {
      try {
        const { getCurrentWebviewWindow } = await import('@tauri-apps/api/webviewWindow');
        const win = getCurrentWebviewWindow();
        const visible = await win.isVisible();
        if (visible) {
          setIsReady(true);
          
          // Overlap phases for fluidity
          // Impact starts as it reaches the bottom
          setTimeout(() => setIsImpacted(true), 280);
          
          // Expand starts almost immediately after impact to look "liquid"
          setTimeout(() => setIsExpanded(true), 350);
          
          return true;
        }
      } catch (e) {}
      return false;
    };

    const interval = setInterval(async () => {
      if (await checkVisibility()) clearInterval(interval);
    }, 100);
    
    checkVisibility();
    return () => clearInterval(interval);
  }, []);

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
  }, [pinnedApps, activeApps, isHidden]);

  useEffect(() => {
    const init = async () => {
      const pinned = await invoke<AppInfo[]>('load_pinned_apps');
      setPinnedApps(pinned.map(a => ({ ...a, is_pinned: true })));
      pinned.forEach(app => fetchIcon(app.path));
    };
    init();

    const unlistenSettings = listen<{ key: string, value: any }>("settings-changed", (event) => {
      if (event.payload.key === "dock-mode") setDockMode(event.payload.value);
    });

    const unlistenOverlap = listen<boolean>("dock-overlap", (event) => {
      setIsOverlapped(event.payload);
    });

    const unlistenEdgeHover = listen<boolean>("dock-edge-hover", (event) => {
      setIsEdgeHovered(event.payload);
    });

    return () => {
      unlistenSettings.then(f => f());
      unlistenOverlap.then(f => f());
      unlistenEdgeHover.then(f => f());
    };
  }, []);

  useEffect(() => {
    const poll = async () => {
      if (isDragging) return;
      const running = await invoke<AppInfo[]>('get_active_windows');
      setActiveApps(running);
      
      setActiveOrder(prev => {
        const newPaths = running.map(r => r.path);
        const existingPaths = prev.filter(p => newPaths.includes(p));
        const addedPaths = newPaths.filter(p => !prev.includes(p));
        return [...existingPaths, ...addedPaths];
      });

      running.forEach(app => fetchIcon(app.path, app.hwnd));
    };

    const interval = setInterval(poll, 2000);
    poll();
    return () => clearInterval(interval);
  }, [isDragging]);

  const fetchIcon = async (path: string, hwnd?: number, retryCount = 0) => {
    const cacheKey = hwnd ? `${path}-${hwnd}` : path;
    if (iconsRef.current[cacheKey]) return;
    try {
      const icon = await invoke<string | null>('get_app_icon', { path, hwnd: hwnd || null });
      if (icon) {
        iconsRef.current[cacheKey] = icon;
        iconsRef.current[path] = icon;
        setIconsTick(t => t + 1);
      } else if (retryCount < 3 && !hwnd) {
        // If it's a pinned app (no hwnd) and failed, retry with backoff
        setTimeout(() => fetchIcon(path, undefined, retryCount + 1), 3000 * (retryCount + 1));
      }
    } catch (e) {
      console.error(`Failed to fetch icon for ${path}:`, e);
      if (retryCount < 3 && !hwnd) {
        setTimeout(() => fetchIcon(path, undefined, retryCount + 1), 3000 * (retryCount + 1));
      }
    }
  };

  const handleAppClick = async (app: AppInfo) => {
    try {
      if (app.path === 'start') {
        await invoke('open_app', { appName: 'start' });
      } else if (app.hwnd) {
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
    setActiveSubmenu(null);
    invoke('set_menu_open', { open: false, rect: null }).catch(() => {});
  };

  const closePopup = () => {
    setShowAddPopup(false);
    invoke('set_menu_open', { open: false, rect: null }).catch(() => {});
  };

  useEffect(() => {
    let open = false;
    let rect = null;

    if (contextMenu && menuRef.current) {
      const r = menuRef.current.getBoundingClientRect();
      rect = { 
        x: Math.round(r.x), 
        y: Math.round(r.y), 
        width: Math.round(r.width + (activeSubmenu ? 160 : 0)), 
        height: Math.round(r.height) 
      };
      open = true;
    } else if (showAddPopup && popupRef.current) {
      const r = popupRef.current.getBoundingClientRect();
      rect = { x: Math.round(r.x), y: Math.round(r.y), width: Math.round(r.width), height: Math.round(r.height) };
      open = true;
    }

    invoke('set_menu_open', { open, rect }).catch(() => {});
  }, [contextMenu, showAddPopup, pinnedApps, activeApps, activeSubmenu]);

  const dockItems = useMemo(() => {
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

  const startItem = useMemo(() => dockItems.find(i => i.path === 'start') as AppInfo, [dockItems]);
  const otherItems = useMemo(() => dockItems.filter(i => i.path !== 'start'), [dockItems]);

  const handleReorder = (newItems: AppInfo[]) => {
    const newPinned = newItems.filter(item => item.is_pinned);
    const newPinnedPaths = newPinned.map(p => p.path);
    const oldPinnedPaths = pinnedApps.map(p => p.path);

    if (JSON.stringify(newPinnedPaths) !== JSON.stringify(oldPinnedPaths)) {
      setPinnedApps(newPinned);
    }

    const newActivePaths = newItems.filter(item => !item.is_pinned).map(item => item.path);
    if (newActivePaths.length > 0 && JSON.stringify(newActivePaths) !== JSON.stringify(activeOrder)) {
      setActiveOrder(newActivePaths);
    }
  };

  const handleDragEnd = () => {
    setIsDragging(false);
    setPressedApp(null);
    invoke('save_pinned_apps', { apps: pinnedApps }).catch(console.error);
  };

  useEffect(() => {
    invoke('set_dock_hovered', { hovered: isCurrentlyHovered }).catch(() => {});
  }, [isCurrentlyHovered]);

  const iconVariants = {
    idle: { y: 0, scale: 1 },
    hover: { y: -5, scale: 1.1 },
    drag: { y: -10, scale: 1.1, opacity: 0.8 },
    tap: { scale: 0.95 }
  };

  return (
    <div className={`dock-container ${isDragging ? 'dragging' : ''}`} onClick={closeMenu}>
      <motion.div
        layout
        ref={dockRef}
        className={`dock ${(isImpacted || isExpanded) && !isHidden ? 'dock-expanded' : ''}`}
        onMouseEnter={() => setIsDockHovered(true)}
        onMouseLeave={() => { setIsDockHovered(false); setHoveredApp(null); setPressedApp(null); }}
        initial={{ y: -800, opacity: 1, width: 34, height: 34, borderTopLeftRadius: 17, borderTopRightRadius: 17, borderBottomLeftRadius: 17, borderBottomRightRadius: 17, scaleX: 0.9, scaleY: 1.3 }}
        animate={{ 
          y: !isReady ? -800 : (isHidden ? 100 : 0), 
          width: isExpanded && !isHidden ? 'auto' : 34,
          height: isExpanded && !isHidden ? 'auto' : 34,
          borderTopLeftRadius: isExpanded && !isHidden ? 18 : 17,
          borderTopRightRadius: isExpanded && !isHidden ? 18 : 17,
          borderBottomLeftRadius: (isImpacted || isExpanded) && !isHidden ? 0 : 17,
          borderBottomRightRadius: (isImpacted || isExpanded) && !isHidden ? 0 : 17,
          scaleX: !isReady ? 1 : (isExpanded ? 1 : (isImpacted ? 1.15 : 0.9)),
          scaleY: !isReady ? 1 : (isExpanded ? 1 : (isImpacted ? 0.85 : 1.3)),
          opacity: 1,
        }}
        transition={{ 
          y: { type: "spring", stiffness: 550, damping: 45, mass: 0.8, restDelta: 0.001 },
          width: { type: "spring", stiffness: 450, damping: 25, mass: 0.8 },
          height: { type: "spring", stiffness: 450, damping: 25, mass: 0.8 },
          scaleX: { type: "spring", stiffness: 500, damping: 20 },
          scaleY: { type: "spring", stiffness: 500, damping: 20 },
          default: { type: "spring", stiffness: 500, damping: 30, mass: 1 }
        }}
        style={{ originY: 1, minWidth: 34 }}
        onContextMenu={(e) => handleContextMenu(e, null)}
      >
        <AnimatePresence mode="wait">
          {isExpanded && (
            <motion.div 
              key="dock-content"
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.8 }}
              className="dock-reorder-container"
              variants={{
                show: { transition: { staggerChildren: 0.04 } }
              }}
            >
              {startItem && (
                <motion.div 
                  variants={{ hide: { opacity: 0, y: 10 }, show: { opacity: 1, y: 0 } }}
                  className="dock-icon-wrapper"
                  onContextMenu={(e) => handleContextMenu(e, startItem)}
                  onMouseEnter={() => setHoveredApp(startItem.path)}
                  onMouseLeave={() => { setHoveredApp(null); setPressedApp(null); }}
                >
                  <div className="tooltip">{startItem.name}</div>
                  <motion.div 
                    className="dock-icon"
                    variants={iconVariants}
                    animate={pressedApp === startItem.path ? "tap" : (hoveredApp === startItem.path ? "hover" : "idle")}
                    onPointerDown={() => setPressedApp(startItem.path)}
                    onPointerUp={() => setPressedApp(null)}
                    onPointerCancel={() => setPressedApp(null)}
                    onClick={(e) => {
                      e.stopPropagation();
                      handleAppClick(startItem);
                    }}
                  >
                    <img src="/bloom.png" alt="Bloom" draggable={false} />
                  </motion.div>
                </motion.div>
              )}
              
              <Reorder.Group
                as="div"
                axis="x"
                values={otherItems}
                onReorder={handleReorder}
                className="dock-reorder-group"
              >
                {otherItems.map((app) => (
                  <Reorder.Item
                    as="div"
                    key={app.path}
                    value={app}
                    dragListener={!!app.is_pinned}
                    layout
                    variants={{ hide: { opacity: 0, y: 10 }, show: { opacity: 1, y: 0 } }}
                    className="dock-icon-wrapper"
                    onContextMenu={(e) => handleContextMenu(e, app)}
                    onMouseEnter={() => setHoveredApp(app.path)}
                    onMouseLeave={() => { setHoveredApp(null); setPressedApp(null); }}
                    onDragStart={() => { if (app.is_pinned) { setIsDragging(true); setHoveredApp(null); setPressedApp(null); } }}
                    onDragEnd={handleDragEnd}
                  >
                <div className="tooltip">{app.name}</div>
                <motion.div 
                  className="dock-icon"
                  variants={iconVariants}
                  animate={pressedApp === app.path ? "tap" : (isDragging && !app.is_pinned ? "idle" : (hoveredApp === app.path && !isDragging ? "hover" : "idle"))}
                  whileDrag="drag"
                  onPointerDown={() => setPressedApp(app.path)}
                  onPointerUp={() => setPressedApp(null)}
                  onPointerCancel={() => setPressedApp(null)}
                  onTap={() => {
                    if (!isDragging) handleAppClick(app);
                  }}
                >
                  {iconsRef.current[app.path] ? (
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
          )}
        </AnimatePresence>
      </motion.div>

      {contextMenu && (
        <div 
          ref={menuRef}
          className="context-menu" 
          style={{ left: contextMenu.x, top: contextMenu.y - (contextMenu.app ? 200 : 100) }}
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
              <div 
                className="menu-item has-submenu"
                onMouseEnter={() => setActiveSubmenu('bloom')}
                onMouseLeave={() => setActiveSubmenu(null)}
              >
                Bloom Options
                <span className="submenu-arrow">▶</span>
                <div className="submenu">
                  <div className="menu-item" onClick={() => { invoke('open_settings_window'); closeMenu(); }}>Open Settings</div>
                  <div className="menu-item" onClick={() => invoke('restart_bloom')}>Restart Bloom</div>
                  <div className="menu-divider" />
                  <div className="menu-item quit" onClick={() => invoke('quit_bloom')}>Quit Bloom</div>
                </div>
              </div>
              {contextMenu.app.is_running && (
                <>
                  <div className="menu-divider" />
                  <div className="menu-item quit" onClick={async () => {
                    if (contextMenu.app?.hwnd) {
                      await invoke('close_window', { hwnd: contextMenu.app.hwnd });
                      closeMenu();
                    }
                  }}>
                    Quit {contextMenu.app.name}
                  </div>
                </>
              )}
            </>
          ) : (
            <>
              <div className="menu-item" onClick={() => { setShowAddPopup(true); closeMenu(); }}>
                Add App to Dock...
              </div>
              <div 
                className="menu-item has-submenu"
                onMouseEnter={() => setActiveSubmenu('bloom')}
                onMouseLeave={() => setActiveSubmenu(null)}
              >
                Bloom Options
                <span className="submenu-arrow">▶</span>
                <div className="submenu">
                  <div className="menu-item" onClick={() => { invoke('open_settings_window'); closeMenu(); }}>Open Settings</div>
                  <div className="menu-item" onClick={() => invoke('restart_bloom')}>Restart Bloom</div>
                  <div className="menu-divider" />
                  <div className="menu-item quit" onClick={() => invoke('quit_bloom')}>Quit Bloom</div>
                </div>
              </div>
            </>
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

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 150);
    return () => clearTimeout(timer);
  }, [search]);

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
    return apps.filter(a => a.name.toLowerCase().includes(s)).slice(0, 50);
  }, [apps, debouncedSearch]);

  useEffect(() => {
    let active = true;
    const fetchVisibleIcons = async () => {
      let batch: Record<string, string> = {};
      let count = 0;
      for (const app of filtered) {
        if (!active) break;
        if (!listIcons[app.path]) {
          await new Promise(r => setTimeout(r, 25)); 
          try {
            const icon = await invoke<string | null>('get_app_icon', { path: app.path });
            if (icon && active) {
              batch[app.path] = icon;
              count++;
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
      if (active && count > 0) setListIcons(prev => ({ ...prev, ...batch }));
    };
    fetchVisibleIcons();
    return () => { active = false; };
  }, [filtered]);

  return (
    <motion.div className="popup-overlay" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} onClick={onClose}>
      <motion.div ref={containerRef} className="add-app-popup" layout initial={{ scale: 0.95, opacity: 0, y: 20 }} animate={{ scale: 1, opacity: 1, y: 0 }} exit={{ scale: 0.95, opacity: 0, y: 20 }} onClick={(e) => e.stopPropagation()}>
        <div className="popup-header">
          <h3>Add to Dock</h3>
          <div className="search-container">
            <input type="text" placeholder="Search applications..." autoFocus value={search} onChange={(e) => setSearch(e.target.value)} />
          </div>
        </div>
        <div className="apps-list">
          {loading ? (
             <div className="loading-state"><div className="spinner"></div><p>Searching for apps...</p></div>
          ) : filtered.length > 0 ? (
            filtered.map(app => (
              <div key={app.path} className="app-list-item" onClick={() => onAdd(app)}>
                <div className="app-list-info">
                  <div className="app-list-icon">
                    {listIcons[app.path] ? <img src={listIcons[app.path]} alt="" draggable={false} /> : <div className="app-icon-placeholder">{app.name[0]}</div>}
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
