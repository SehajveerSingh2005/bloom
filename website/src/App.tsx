import { useState, useEffect, useCallback } from 'react';

import Notch from './components/Notch';
import Dock from './components/Dock';
import Window from './components/Window';

import AboutApp from './components/apps/AboutApp';
import MusicApp from './components/apps/MusicApp';
import SettingsApp from './components/apps/SettingsApp';
import TerminalApp from './components/apps/TerminalApp';
import ChangelogApp from './components/apps/ChangelogApp';
import PerformanceApp from './components/apps/PerformanceApp';
import FeaturesApp from './components/apps/FeaturesApp';
import BrowserApp from './components/apps/BrowserApp';

import wallpaperImg from './assets/wallpaper.jpg';
import wallpaper2 from './assets/wallpaper-2.png';
import wallpaper3 from './assets/wallpaper-3.jpg';
import wallpaper4 from './assets/wallpaper-4.jpg';

const wallpapersList = [
  wallpaperImg,
  wallpaper2,
  wallpaper3,
  wallpaper4,
];

export default function App() {
  const [settings, setSettings] = useState(() => {
    const saved = localStorage.getItem('bloom-settings');
    if (saved) {
      try { return JSON.parse(saved); } catch {}
    }
    return {
      wallpaper: 2,
      dockMode: 'fixed' as 'fixed' | 'auto-hide',
      notchMode: 'fixed' as 'fixed' | 'auto-hide',
      accentColor: '#e8c5e5',
      isDockEnabled: true,
    };
  });

  const [openApps, setOpenApps] = useState<string[]>(['about', 'music', 'terminal']);
  const [minimizedApps, setMinimizedApps] = useState<string[]>([]);
  const [focusedApp, setFocusedApp] = useState<string>('about');

  const [positions, setPositions] = useState({
    about: { x: 98, y: 117 },
    music: { x: 716, y: 42 },
    terminal: { x: 527, y: 335 },
    settings: { x: 140, y: 130 },
    changelog: { x: 300, y: 80 },
    performance: { x: 850, y: 250 },
    features: { x: 200, y: 60 },
    browser: { x: 400, y: 80 },
  });

  const [viewport, setViewport] = useState({ w: window.innerWidth, h: window.innerHeight });
  useEffect(() => {
    const onResize = () => setViewport({ w: window.innerWidth, h: window.innerHeight });
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  // Clamp positions when viewport shrinks so windows stay on screen
  useEffect(() => {
    setPositions((prev) => {
      const clamp = (x: number, y: number, w: number, h: number) => ({
        x: Math.max(16, Math.min(x, viewport.w - w - 16)),
        y: Math.max(12, Math.min(y, viewport.h - h - 80)),
      });
      return {
        about: clamp(prev.about.x, prev.about.y, 500, 460),
        music: clamp(prev.music.x, prev.music.y, 680, 400),
        terminal: clamp(prev.terminal.x, prev.terminal.y, 500, 320),
        settings: clamp(prev.settings.x, prev.settings.y, 520, 480),
        changelog: clamp(prev.changelog.x, prev.changelog.y, 480, 560),
        performance: clamp(prev.performance.x, prev.performance.y, 420, 440),
        features: clamp(prev.features.x, prev.features.y, 380, 420),
        browser: clamp(prev.browser.x, prev.browser.y, 640, 450),
      };
    });
  }, [viewport.w, viewport.h]);

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

  const [visualizerData, setVisualizerData] = useState<number[]>([0.15, 0.15, 0.15, 0.15, 0.15]);

  const updateSetting = useCallback((key: string, value: any) => {
    setSettings((prev) => ({ ...prev, [key]: value }));
  }, []);

  const setPlaybackState = useCallback((state: Partial<typeof playback>) => {
    setPlayback((prev) => ({ ...prev, ...state }));
  }, []);

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--accent-color', settings.accentColor);
    root.style.setProperty('--accent-hover', settings.accentColor + 'cc');
    root.style.setProperty('--bg-glow', `${settings.accentColor}18`);
    localStorage.setItem('bloom-settings', JSON.stringify(settings));
  }, [settings]);

  const handleOpenApp = useCallback((appId: string) => {
    setOpenApps((prev) => prev.includes(appId) ? prev : [...prev, appId]);
    setMinimizedApps((prev) => prev.filter((id) => id !== appId));
    setFocusedApp(appId);
  }, []);

  const handleCloseApp = useCallback((appId: string) => {
    setOpenApps((prev) => prev.filter((id) => id !== appId));
    setMinimizedApps((prev) => prev.filter((id) => id !== appId));
    setFocusedApp((prev) => {
      if (prev !== appId) return prev;
      return '';
    });
  }, []);

  const handleMinimizeApp = useCallback((appId: string) => {
    setMinimizedApps((prev) => prev.includes(appId) ? prev : [...prev, appId]);
    setFocusedApp((prev) => (prev === appId ? '' : prev));
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.altKey && e.key.toLowerCase() === 't') { e.preventDefault(); handleOpenApp('terminal'); }
      if (e.altKey && e.key.toLowerCase() === 'c') { e.preventDefault(); handleOpenApp('changelog'); }
      if (e.altKey && e.key.toLowerCase() === 'p') { e.preventDefault(); handleOpenApp('performance'); }
      if (e.altKey && e.key.toLowerCase() === 'f') { e.preventDefault(); handleOpenApp('features'); }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleOpenApp]);

  const activeWallpaperUrl = wallpapersList[settings.wallpaper] || wallpapersList[0];

  return (
    <div
      className="relative w-screen h-screen overflow-hidden select-none"
      style={{
        backgroundImage: `url(${activeWallpaperUrl})`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
        transition: 'background-image 0.5s ease-in-out',
      }}
    >
      <div
        className="absolute inset-0 glow-accent pointer-events-none transition-all duration-500"
        style={{ opacity: playback.isPlaying ? 1 : 0 }}
      />

      <Notch
        settings={settings}
        playback={playback}
        setPlaybackState={setPlaybackState}
        visualizerData={visualizerData}
        onOpenApp={handleOpenApp}
        updateSetting={updateSetting}
      />

      {/* Windows */}
      <div className="absolute inset-0 pt-10 pb-16 px-2 z-20 pointer-events-none">
        <div className="relative w-full h-full pointer-events-auto">
          <Window
            id="about"
            title="About Bloom"
            isOpen={openApps.includes('about')}
            isFocused={focusedApp === 'about'}
            isMinimized={minimizedApps.includes('about')}
            onClose={() => handleCloseApp('about')}
            onMinimize={() => handleMinimizeApp('about')}
            onFocus={() => setFocusedApp('about')}
            width={500}
            height={460}
            defaultPosition={positions.about}
            viewport={viewport}
          >
            <AboutApp
              githubUrl="https://github.com/SehajveerSingh2005/bloom"
              downloadUrl="https://github.com/SehajveerSingh2005/bloom/releases/latest"
              accentColor={settings.accentColor}
              onOpenApp={handleOpenApp}
            />
          </Window>

          <Window
            id="music"
            title="Music Player"
            isOpen={openApps.includes('music')}
            isFocused={focusedApp === 'music'}
            isMinimized={minimizedApps.includes('music')}
            onClose={() => handleCloseApp('music')}
            onMinimize={() => handleMinimizeApp('music')}
            onFocus={() => setFocusedApp('music')}
            width={680}
            height={400}
            defaultPosition={positions.music}
            viewport={viewport}
          >
            <MusicApp
              playback={playback}
              setPlaybackState={setPlaybackState}
              setVisualizerData={setVisualizerData}
            />
          </Window>

          <Window
            id="settings"
            title="Settings"
            isOpen={openApps.includes('settings')}
            isFocused={focusedApp === 'settings'}
            isMinimized={minimizedApps.includes('settings')}
            onClose={() => handleCloseApp('settings')}
            onMinimize={() => handleMinimizeApp('settings')}
            onFocus={() => setFocusedApp('settings')}
            width={520}
            height={480}
            defaultPosition={positions.settings}
            viewport={viewport}
          >
            <SettingsApp
              settings={settings}
              updateSetting={updateSetting}
              wallpapersList={wallpapersList}
            />
          </Window>

          <Window
            id="terminal"
            title="bloom-system-daemon"
            isOpen={openApps.includes('terminal')}
            isFocused={focusedApp === 'terminal'}
            isMinimized={minimizedApps.includes('terminal')}
            onClose={() => handleCloseApp('terminal')}
            onMinimize={() => handleMinimizeApp('terminal')}
            onFocus={() => setFocusedApp('terminal')}
            width={500}
            height={320}
            defaultPosition={positions.terminal}
            viewport={viewport}
          >
            <TerminalApp accentColor={settings.accentColor} />
          </Window>

          <Window
            id="changelog"
            title="Changelog"
            isOpen={openApps.includes('changelog')}
            isFocused={focusedApp === 'changelog'}
            isMinimized={minimizedApps.includes('changelog')}
            onClose={() => handleCloseApp('changelog')}
            onMinimize={() => handleMinimizeApp('changelog')}
            onFocus={() => setFocusedApp('changelog')}
            width={480}
            height={560}
            defaultPosition={positions.changelog}
            viewport={viewport}
          >
            <ChangelogApp />
          </Window>

          <Window
            id="performance"
            title="Performance Monitor"
            isOpen={openApps.includes('performance')}
            isFocused={focusedApp === 'performance'}
            isMinimized={minimizedApps.includes('performance')}
            onClose={() => handleCloseApp('performance')}
            onMinimize={() => handleMinimizeApp('performance')}
            onFocus={() => setFocusedApp('performance')}
            width={420}
            height={440}
            defaultPosition={positions.performance}
            viewport={viewport}
          >
            <PerformanceApp />
          </Window>

          <Window
            id="features"
            title="Features"
            isOpen={openApps.includes('features')}
            isFocused={focusedApp === 'features'}
            isMinimized={minimizedApps.includes('features')}
            onClose={() => handleCloseApp('features')}
            onMinimize={() => handleMinimizeApp('features')}
            onFocus={() => setFocusedApp('features')}
            width={380}
            height={420}
            defaultPosition={positions.features}
            viewport={viewport}
          >
            <FeaturesApp />
          </Window>

          <Window
            id="browser"
            title="Browser"
            isOpen={openApps.includes('browser')}
            isFocused={focusedApp === 'browser'}
            isMinimized={minimizedApps.includes('browser')}
            onClose={() => handleCloseApp('browser')}
            onMinimize={() => handleMinimizeApp('browser')}
            onFocus={() => setFocusedApp('browser')}
            width={640}
            height={450}
            defaultPosition={positions.browser}
            viewport={viewport}
          >
            <BrowserApp />
          </Window>
        </div>
      </div>

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
