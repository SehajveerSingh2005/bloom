import { useState, useEffect } from 'react';

import Notch from './components/Notch';
import Dock from './components/Dock';
import Window from './components/Window';

// Import apps
import AboutApp from './components/apps/AboutApp';
import MusicApp from './components/apps/MusicApp';
import SettingsApp from './components/apps/SettingsApp';
import TerminalApp from './components/apps/TerminalApp';

import wallpaperImg from './assets/wallpaper.jpg';

const wallpapersList = [
  wallpaperImg,
  'https://images.unsplash.com/photo-1579546929518-9e396f3cc809?q=80&w=1920&auto=format&fit=crop', // Purple Gradient
  'https://images.unsplash.com/photo-1618005182384-a83a8bd57fbe?q=80&w=1920&auto=format&fit=crop', // Forest Abstract
  'https://images.unsplash.com/photo-1506318137071-a8e063b4bec0?q=80&w=1920&auto=format&fit=crop', // Cosmic Space
  'https://images.unsplash.com/photo-1634017839464-5c339ebe3cb4?q=80&w=1920&auto=format&fit=crop', // Soft minimal clean
];

export default function App() {
  // Simulator OS State
  const [settings, setSettings] = useState({
    wallpaper: 0,
    dockMode: 'fixed' as 'fixed' | 'auto-hide',
    notchMode: 'fixed' as 'fixed' | 'auto-hide',
    accentColor: '#e8c5e5',
    isDockEnabled: true,
  });

  const [openApps, setOpenApps] = useState<string[]>(['about', 'music', 'terminal']);
  const [minimizedApps, setMinimizedApps] = useState<string[]>([]);
  const [focusedApp, setFocusedApp] = useState<string>('about');

  // Responsive initial window positions
  const [positions, setPositions] = useState({
    about: { x: 98, y: 117 },
    music: { x: 716, y: 42 },
    terminal: { x: 527, y: 335 },
    settings: { x: 140, y: 130 }
  });

  useEffect(() => {
    const handleViewportInit = () => {
      const w = window.innerWidth;
      const h = window.innerHeight;
      const getPos = (dx: number, dy: number, winW: number, winH: number) => {
        const rx = Math.max(16, Math.min(dx, w - winW - 16));
        const ry = Math.max(56, Math.min(dy, h - winH - 16));
        return { x: rx, y: ry };
      };
      setPositions({
        about: getPos(98, 117, 500, 520),
        music: getPos(716, 42, 680, 400),
        terminal: getPos(527, 335, 500, 320),
        settings: getPos(140, 130, 520, 480)
      });
    };
    handleViewportInit();
    window.addEventListener('resize', handleViewportInit);
    return () => window.removeEventListener('resize', handleViewportInit);
  }, []);

  // Music state
  const [playback, setPlayback] = useState({
    isPlaying: false,
    trackTitle: 'Golden Hour Bloom',
    trackArtist: 'Aesthetic Lo-Fi',
    trackCover: 'https://images.unsplash.com/photo-1518609878373-06d740f60d8b?w=300&auto=format&fit=crop&q=60',
    currentTime: 0,
    duration: 180,
    volume: 0.5,
    trackIndex: 0,
  });

  // Audio frequencies data for notch visualizer
  const [visualizerData, setVisualizerData] = useState<number[]>([0.15, 0.15, 0.15, 0.15, 0.15]);

  const updateSetting = (key: string, value: any) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  };

  const setPlaybackState = (state: Partial<typeof playback>) => {
    setPlayback((prev) => ({ ...prev, ...state }));
  };

  // Inject accent color variables into css
  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--accent-color', settings.accentColor);

    // Calculate lighter hover color
    const hoverColor = settings.accentColor + 'cc';
    root.style.setProperty('--accent-hover', hoverColor);

    // Calculate background aura glow
    root.style.setProperty('--bg-glow', `${settings.accentColor}18`);
  }, [settings.accentColor]);

  // Handle open/focus app actions
  const handleOpenApp = (appId: string) => {
    // If not open, add to open list
    if (!openApps.includes(appId)) {
      setOpenApps((prev) => [...prev, appId]);
    }
    // If minimized, restore it
    if (minimizedApps.includes(appId)) {
      setMinimizedApps((prev) => prev.filter((id) => id !== appId));
    }
    // Focus it
    setFocusedApp(appId);
  };

  const handleCloseApp = (appId: string) => {
    setOpenApps((prev) => prev.filter((id) => id !== appId));
    setMinimizedApps((prev) => prev.filter((id) => id !== appId));
    if (focusedApp === appId) {
      const remaining = openApps.filter((id) => id !== appId);
      if (remaining.length > 0) {
        setFocusedApp(remaining[remaining.length - 1]);
      } else {
        setFocusedApp('');
      }
    }
  };

  const handleMinimizeApp = (appId: string) => {
    if (!minimizedApps.includes(appId)) {
      setMinimizedApps((prev) => [...prev, appId]);
    }
    // Unfocus
    if (focusedApp === appId) {
      const remaining = openApps.filter((id) => id !== appId && !minimizedApps.includes(id));
      if (remaining.length > 0) {
        setFocusedApp(remaining[remaining.length - 1]);
      } else {
        setFocusedApp('');
      }
    }
  };

  // Keyboard Shortcuts (Alt + T to launch terminal, Escape to minimize current app)
  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.altKey && e.key.toLowerCase() === 't') {
        e.preventDefault();
        handleOpenApp('terminal');
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [openApps, minimizedApps]);

  const activeWallpaperUrl = wallpapersList[settings.wallpaper] || wallpapersList[0];

  return (
    <div
      className="relative w-screen h-screen overflow-hidden flex flex-col items-center justify-between select-none"
      style={{
        backgroundImage: `url(${activeWallpaperUrl})`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
        transition: 'background-image 0.5s ease-in-out',
      }}
    >
      {/* Dynamic Background Accent Glow */}
      <div
        className="absolute inset-0 glow-accent pointer-events-none transition-all duration-500"
        style={{ opacity: playback.isPlaying ? 1 : 0 }}
      />

      {/* Top Bar / Notch */}
      <Notch
        settings={settings}
        playback={playback}
        setPlaybackState={setPlaybackState}
        visualizerData={visualizerData}
        onOpenApp={handleOpenApp}
        updateSetting={updateSetting}
      />

      {/* OS Windows Stack */}
      <div className="absolute inset-0 pt-14 pb-20 px-6 z-20 pointer-events-none">
        <div className="relative w-full h-full pointer-events-auto">
          {/* About App Window */}
          <Window
            id="about"
            title="About Bloom"
            isOpen={openApps.includes('about')}
            isFocused={focusedApp === 'about'}
            isMinimized={minimizedApps.includes('about')}
            onClose={() => handleCloseApp('about')}
            onMinimize={() => handleMinimizeApp('about')}
            onFocus={() => setFocusedApp('about')}
            width="w-[500px]"
            height="h-[520px]"
            defaultPosition={positions.about}
          >
            <AboutApp
              githubUrl="https://github.com/SehajveerSingh2005/bloom"
              downloadUrl="https://github.com/SehajveerSingh2005/bloom/releases/latest"
              accentColor={settings.accentColor}
              onOpenApp={handleOpenApp}
            />
          </Window>

          {/* Music App Window */}
          <Window
            id="music"
            title="Music Player"
            isOpen={openApps.includes('music')}
            isFocused={focusedApp === 'music'}
            isMinimized={minimizedApps.includes('music')}
            onClose={() => handleCloseApp('music')}
            onMinimize={() => handleMinimizeApp('music')}
            onFocus={() => setFocusedApp('music')}
            width="w-[680px]"
            height="h-[400px]"
            defaultPosition={positions.music}
          >
            <MusicApp
              playback={playback}
              setPlaybackState={setPlaybackState}
              setVisualizerData={setVisualizerData}
            />
          </Window>

          {/* Settings App Window */}
          <Window
            id="settings"
            title="Settings"
            isOpen={openApps.includes('settings')}
            isFocused={focusedApp === 'settings'}
            isMinimized={minimizedApps.includes('settings')}
            onClose={() => handleCloseApp('settings')}
            onMinimize={() => handleMinimizeApp('settings')}
            onFocus={() => setFocusedApp('settings')}
            width="w-[520px]"
            height="h-[480px]"
            defaultPosition={positions.settings}
          >
            <SettingsApp
              settings={settings}
              updateSetting={updateSetting}
              wallpapersList={wallpapersList}
            />
          </Window>

          {/* Developer Terminal Logs App Window */}
          <Window
            id="terminal"
            title="bloom-system-daemon"
            isOpen={openApps.includes('terminal')}
            isFocused={focusedApp === 'terminal'}
            isMinimized={minimizedApps.includes('terminal')}
            onClose={() => handleCloseApp('terminal')}
            onMinimize={() => handleMinimizeApp('terminal')}
            onFocus={() => setFocusedApp('terminal')}
            width="w-[500px]"
            height="h-[320px]"
            defaultPosition={positions.terminal}
          >
            <TerminalApp accentColor={settings.accentColor} />
          </Window>
        </div>
      </div>

      {/* Dock Bar */}
      <Dock
        settings={settings}
        openApps={openApps}
        minimizedApps={minimizedApps}
        onOpenApp={handleOpenApp}
        onCloseApp={handleCloseApp}
      />
    </div>
  );
}
