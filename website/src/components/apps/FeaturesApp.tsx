import { useState } from 'react';
import { Sparkles, Zap, Music, Palette, Layout, Shield, ChevronRight } from 'lucide-react';

interface Feature {
  id: string;
  icon: React.ReactNode;
  title: string;
  desc: string;
  color: string;
  detail: string;
}

const features: Feature[] = [
  {
    id: 'spring',
    icon: <Zap size={14} />,
    title: 'Spring Physics',
    desc: 'Real-time spring animations, no easing curves',
    color: '#fbbf24',
    detail: 'Every window, menu, and transition uses live spring physics simulations instead of pre-defined bezier curves. Motion feels organic and responsive to user intent.',
  },
  {
    id: 'shell',
    icon: <Layout size={14} />,
    title: 'Shell Control',
    desc: 'Replaces Explorer taskbar at the OS layer',
    color: '#34d399',
    detail: 'Bloom silences the legacy Windows taskbar and dock at the system level, giving you full control over the desktop shell without modifying core OS files.',
  },
  {
    id: 'music',
    icon: <Music size={14} />,
    title: 'Music Integration',
    desc: 'System-wide media controls with live visualizer',
    color: '#f472b6',
    detail: 'Built-in music player with real-time audio visualization, album art ambient glow, and system media key integration. Supports any audio source.',
  },
  {
    id: 'themes',
    icon: <Palette size={14} />,
    title: 'Dynamic Theming',
    desc: 'Accent colors propagate across the entire UI',
    color: '#a78bfa',
    detail: 'Choose any accent color and watch it ripple through windows, the dock, notch, and all UI elements. Glassmorphism adapts to your wallpaper.',
  },
  {
    id: 'notch',
    icon: <Sparkles size={14} />,
    title: 'Dynamic Notch',
    desc: 'Multi-mode widget: music, calendar, controls',
    color: '#60a5fa',
    detail: 'A floating notch at the top of your screen that expands into a music player, calendar with timer, or command center. Scroll to switch modes.',
  },
  {
    id: 'performance',
    icon: <Shield size={14} />,
    title: 'Rust Core',
    desc: 'Under 10MB idle RAM, Tauri v2 architecture',
    color: '#fb923c',
    detail: 'The core is written in Rust with a Tauri v2 bridge. Multiple process layers coordinate under 10MB idle RAM usage with near-zero CPU overhead.',
  },
];

export default function FeaturesApp() {
  const [selected, setSelected] = useState<string | null>(null);
  const activeFeature = features.find((f) => f.id === selected);

  return (
    <div className="h-full w-full bg-transparent text-white flex flex-col select-none overflow-hidden font-sans">
      {/* Header */}
      <div className="px-5 pt-4 pb-3 shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-5 h-5 rounded-md bg-violet-500/15 flex items-center justify-center">
            <Sparkles size={11} className="text-violet-400" />
          </div>
          <span className="text-[11px] uppercase tracking-widest font-bold text-white/40">Features</span>
        </div>
        <p className="text-[9px] text-white/25 mt-1">What makes Bloom different</p>
      </div>

      <div className="flex-1 overflow-y-auto px-5 pb-5">
        {selected && activeFeature ? (
          /* Detail view */
          <div>
            <button
              onClick={() => setSelected(null)}
              className="flex items-center gap-1 text-[9px] text-white/30 hover:text-white/60 transition-colors mb-3 cursor-pointer"
            >
              <ChevronRight size={10} className="rotate-180" />
              All features
            </button>
            <div
              className="w-8 h-8 rounded-xl flex items-center justify-center mb-3"
              style={{ background: `${activeFeature.color}18`, color: activeFeature.color }}
            >
              {activeFeature.icon}
            </div>
            <h3 className="text-[14px] font-bold text-white/90 mb-1">{activeFeature.title}</h3>
            <p className="text-[10px] text-white/40 leading-relaxed">{activeFeature.detail}</p>
          </div>
        ) : (
          /* Grid view */
          <div className="space-y-2">
            {features.map((f) => (
              <button
                key={f.id}
                onClick={() => setSelected(f.id)}
                className="w-full flex items-center gap-3 p-3 rounded-xl bg-white/[0.02] border border-white/[0.04] hover:bg-white/[0.05] hover:border-white/[0.08] transition-all text-left cursor-pointer group"
              >
                <div
                  className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0"
                  style={{ background: `${f.color}12`, color: f.color }}
                >
                  {f.icon}
                </div>
                <div className="flex-1 min-w-0">
                  <span className="text-[11px] font-semibold text-white/80 block">{f.title}</span>
                  <span className="text-[9px] text-white/30 block truncate">{f.desc}</span>
                </div>
                <ChevronRight size={12} className="text-white/15 group-hover:text-white/30 transition-colors shrink-0" />
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
