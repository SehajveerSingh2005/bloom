import { useRef, useState } from 'react';
import { Sparkles, Zap, Music, Palette, Layout, Cpu } from 'lucide-react';

interface Feature {
  id: string;
  icon: React.ReactNode;
  title: string;
  desc: string;
  detail: string;
  color: string;
}

const features: Feature[] = [
  {
    id: 'spring',
    icon: <Zap size={20} />,
    title: 'Spring Physics',
    desc: 'Real-time spring animations',
    detail: 'Every window and transition uses live spring physics instead of bezier curves. Motion feels organic.',
    color: '#fbbf24',
  },
  {
    id: 'shell',
    icon: <Layout size={20} />,
    title: 'Shell Control',
    desc: 'Replaces Explorer taskbar',
    detail: 'Silences the legacy Windows taskbar at the system level. Full shell control without modifying OS files.',
    color: '#34d399',
  },
  {
    id: 'music',
    icon: <Music size={20} />,
    title: 'Music Player',
    desc: 'Live visualizer & controls',
    detail: 'Real-time audio visualization, album art glow, and system media key integration. Supports any source.',
    color: '#f472b6',
  },
  {
    id: 'themes',
    icon: <Palette size={20} />,
    title: 'Dynamic Theming',
    desc: 'Accent colors everywhere',
    detail: 'Choose any accent color and watch it ripple through windows, dock, notch, and all UI elements.',
    color: '#a78bfa',
  },
  {
    id: 'notch',
    icon: <Sparkles size={20} />,
    title: 'Dynamic Notch',
    desc: 'Multi-mode top widget',
    detail: 'Floating notch that expands into music player, calendar, or command center. Scroll to switch modes.',
    color: '#60a5fa',
  },
  {
    id: 'perf',
    icon: <Cpu size={20} />,
    title: 'Rust Core',
    desc: 'Under 10MB idle RAM',
    detail: 'Written in Rust with Tauri v2. Multiple process layers under 10MB idle RAM with near-zero CPU overhead.',
    color: '#fb923c',
  },
];

function FeatureCard({ feature }: { feature: Feature }) {
  const cardRef = useRef<HTMLDivElement>(null);
  const [mousePos, setMousePos] = useState({ x: 0, y: 0 });
  const [isHovered, setIsHovered] = useState(false);

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!cardRef.current) return;
    const rect = cardRef.current.getBoundingClientRect();
    setMousePos({
      x: e.clientX - rect.left,
      y: e.clientY - rect.top,
    });
  };

  return (
    <div
      ref={cardRef}
      onMouseMove={handleMouseMove}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
      className="relative rounded-2xl border border-white/[0.06] overflow-hidden cursor-default group"
      style={{ background: 'rgba(255,255,255,0.02)' }}
    >
      {/* Spotlight glow following cursor */}
      <div
        className="absolute inset-0 opacity-0 group-hover:opacity-100 transition-opacity duration-300 pointer-events-none"
        style={{
          background: `radial-gradient(300px circle at ${mousePos.x}px ${mousePos.y}px, ${feature.color}18, transparent 70%)`,
        }}
      />

      {/* Border glow on hover */}
      <div
        className="absolute inset-0 rounded-2xl opacity-0 group-hover:opacity-100 transition-opacity duration-300 pointer-events-none"
        style={{
          boxShadow: `inset 0 0 0 1px ${feature.color}30`,
        }}
      />

      <div className="relative z-10 p-5">
        {/* Icon */}
        <div
          className="w-10 h-10 rounded-xl flex items-center justify-center mb-3 transition-transform duration-300 group-hover:scale-110"
          style={{ background: `${feature.color}15`, color: feature.color }}
        >
          {feature.icon}
        </div>

        {/* Title */}
        <h3 className="text-[14px] font-bold text-white/90 mb-0.5">{feature.title}</h3>

        {/* Short desc - always visible */}
        <p className="text-[11px] text-white/35 mb-0">{feature.desc}</p>

        {/* Detail - reveals on hover */}
        <div
          className="overflow-hidden transition-all duration-300 ease-out"
          style={{
            maxHeight: isHovered ? '80px' : '0px',
            opacity: isHovered ? 1 : 0,
            marginTop: isHovered ? '8px' : '0px',
          }}
        >
          <p className="text-[11px] text-white/50 leading-relaxed border-t border-white/[0.06] pt-2.5">
            {feature.detail}
          </p>
        </div>
      </div>
    </div>
  );
}

export default function FeaturesApp() {
  return (
    <div className="h-full w-full bg-transparent text-white flex flex-col select-none overflow-hidden font-sans">
      {/* Header */}
      <div className="px-6 pt-5 pb-3 shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-xl bg-violet-500/15 flex items-center justify-center">
            <Sparkles size={13} className="text-violet-400" />
          </div>
          <div>
            <h1 className="text-[16px] font-bold text-white/90">Features</h1>
            <p className="text-[10px] text-white/30">Hover to explore</p>
          </div>
        </div>
      </div>

      {/* Grid */}
      <div className="flex-1 overflow-y-auto px-6 pb-5">
        <div className="grid grid-cols-2 gap-3">
          {features.map((f) => (
            <FeatureCard key={f.id} feature={f} />
          ))}
        </div>
      </div>
    </div>
  );
}
