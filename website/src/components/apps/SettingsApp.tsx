
interface SettingsAppProps {
  settings: {
    wallpaper: number;
    dockMode: 'fixed' | 'auto-hide';
    notchMode: 'fixed' | 'auto-hide';
    accentColor: string;
    isDockEnabled: boolean;
  };
  updateSetting: (key: string, value: any) => void;
  wallpapersList: string[];
}

export default function SettingsApp({ settings, updateSetting, wallpapersList }: SettingsAppProps) {
  const accentColors = [
    { name: 'Bloom Lavender', hex: '#e8c5e5' },
    { name: 'Cyan Tech', hex: '#00F0FF' },
    { name: 'Sage Forest', hex: '#A4C3B2' },
    { name: 'Vibrant Amber', hex: '#FFB800' },
    { name: 'Nordic Crimson', hex: '#FF453A' },
  ];

  return (
    <div className="flex flex-col h-full text-white/90 select-none text-sm font-sans">
      <div className="flex-1 overflow-y-auto px-5 py-4 space-y-5">
        
        {/* Theme Settings */}
        <div>
          <div className="text-[11px] font-semibold text-white/40 uppercase tracking-wider mb-2">Wallpaper & Accents</div>
          <div className="bg-white/[0.03] border border-white/[0.05] rounded-xl p-3 space-y-4">
            
            {/* Wallpapers Presets */}
            <div>
              <label className="text-[12px] text-white/50 block mb-2">Desktop Wallpaper</label>
              <div className="grid grid-cols-4 gap-2">
                {wallpapersList.map((wp, idx) => (
                  <button
                    key={idx}
                    onClick={() => updateSetting('wallpaper', idx)}
                    className={`relative aspect-[16/10] rounded-md overflow-hidden border-2 transition-all ${
                      settings.wallpaper === idx 
                        ? 'border-white scale-[0.98]' 
                        : 'border-transparent opacity-60 hover:opacity-100 hover:scale-[1.02]'
                    }`}
                  >
                    <div 
                      className="w-full h-full bg-cover bg-center" 
                      style={{ backgroundImage: `url(${wp})` }}
                    />
                    <div className="absolute inset-0 bg-black/10 hover:bg-black/0 transition-colors" />
                  </button>
                ))}
              </div>
            </div>

            <div className="h-[1px] bg-white/5" />

            {/* Accent Color Pickers */}
            <div>
              <label className="text-[12px] text-white/50 block mb-2">System Accent Accent</label>
              <div className="flex items-center gap-3">
                {accentColors.map((color) => (
                  <button
                    key={color.hex}
                    onClick={() => updateSetting('accentColor', color.hex)}
                    className={`w-6 h-6 rounded-full border-2 transition-all relative flex items-center justify-center ${
                      settings.accentColor === color.hex 
                        ? 'border-white scale-[1.05] shadow-[0_0_10px_rgba(255,255,255,0.3)]' 
                        : 'border-transparent opacity-60 hover:opacity-100'
                    }`}
                    style={{ backgroundColor: color.hex }}
                    title={color.name}
                  >
                    {settings.accentColor === color.hex && (
                      <span className="w-1.5 h-1.5 bg-black rounded-full" />
                    )}
                  </button>
                ))}
              </div>
            </div>

          </div>
        </div>

        {/* Desktop Interface Rules */}
        <div>
          <div className="text-[11px] font-semibold text-white/40 uppercase tracking-wider mb-2">Desktop Elements</div>
          <div className="bg-white/[0.03] border border-white/[0.05] rounded-xl divide-y divide-white/5">
            
            {/* Notch Behavior */}
            <div className="flex items-center justify-between p-3">
              <div className="flex flex-col">
                <span className="font-medium text-white/90">Notch Auto-Hide</span>
                <span className="text-[11px] text-white/40">Collapse top bar when not active</span>
              </div>
              <label className="relative inline-flex items-center cursor-pointer">
                <input 
                  type="checkbox" 
                  checked={settings.notchMode === 'auto-hide'} 
                  onChange={(e) => updateSetting('notchMode', e.target.checked ? 'auto-hide' : 'fixed')} 
                  className="sr-only peer"
                />
                <div className="w-9 h-5 bg-white/10 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-white/40"></div>
              </label>
            </div>

            {/* Dock Visibility */}
            <div className="flex items-center justify-between p-3">
              <div className="flex flex-col">
                <span className="font-medium text-white/90">Show Bloom Dock</span>
                <span className="text-[11px] text-white/40">Display the custom application bar</span>
              </div>
              <label className="relative inline-flex items-center cursor-pointer">
                <input 
                  type="checkbox" 
                  checked={settings.isDockEnabled} 
                  onChange={(e) => updateSetting('isDockEnabled', e.target.checked)} 
                  className="sr-only peer"
                />
                <div className="w-9 h-5 bg-white/10 peer-focus:outline-none rounded-full peer peer-checked:after:translate-x-full peer-checked:after:border-white after:content-[''] after:absolute after:top-[2px] after:left-[2px] after:bg-white after:border-gray-300 after:border after:rounded-full after:h-4 after:w-4 after:transition-all peer-checked:bg-white/40"></div>
              </label>
            </div>

            {/* Dock Mode */}
            {settings.isDockEnabled && (
              <div className="flex items-center justify-between p-3">
                <div className="flex flex-col">
                  <span className="font-medium text-white/90">Dock Behavior</span>
                  <span className="text-[11px] text-white/40">Choose auto-hide behavior</span>
                </div>
                <select
                  value={settings.dockMode}
                  onChange={(e) => updateSetting('dockMode', e.target.value)}
                  className="bg-black/40 border border-white/10 rounded px-2 py-1 text-[12px] focus:outline-none focus:border-white/30 text-white"
                >
                  <option value="fixed">Fixed (Always On)</option>
                  <option value="auto-hide">Auto Hide</option>
                </select>
              </div>
            )}

          </div>
        </div>

      </div>
      
      {/* Bottom Footer Info */}
      <div className="px-5 py-3 border-t border-white/[0.05] bg-white/[0.01] flex justify-between items-center text-[10px] text-white/30">
        <span>Bloom Simulation Website v1.0.0</span>
        <span>Made with React & Framer Motion</span>
      </div>
    </div>
  );
}
