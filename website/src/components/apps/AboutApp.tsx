import { useState, useEffect } from 'react';
import { Download, ArrowUpRight } from 'lucide-react';
import TextPressure from '../TextPressure';

interface AboutAppProps {
  githubUrl: string;
  downloadUrl: string;
  accentColor: string;
  onOpenApp: (appId: string) => void;
}

export default function AboutApp({ githubUrl, downloadUrl, accentColor }: AboutAppProps) {
  const [releaseInfo, setReleaseInfo] = useState({
    version: 'v3.8.3',
    downloadUrl: downloadUrl,
  });

  useEffect(() => {
    fetch('https://api.github.com/repos/SehajveerSingh2005/bloom/releases/latest')
      .then((res) => res.json())
      .then((data) => {
        if (data?.tag_name) {
          const asset = data.assets?.find((a: any) =>
            a.name.endsWith('.exe') || a.name.endsWith('.msi') || a.name.endsWith('.zip')
          );
          setReleaseInfo({
            version: data.tag_name,
            downloadUrl: asset?.browser_download_url || downloadUrl,
          });
        }
      })
      .catch(() => {});
  }, []);

  return (
    <div className="relative h-full w-full bg-transparent text-white flex flex-col select-none font-sans">
      <style>{`
        @keyframes aboutGlow {
          0%, 100% { opacity: 0.4; }
          50% { opacity: 0.7; }
        }
      `}</style>

      {/* Background glow */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden">
        <div
          className="absolute top-[30%] left-1/2 -translate-x-1/2 -translate-y-1/2 w-[350px] h-[350px] rounded-full"
          style={{
            background: `radial-gradient(circle, ${accentColor}12 0%, transparent 65%)`,
            filter: 'blur(70px)',
            animation: 'aboutGlow 5s ease-in-out infinite',
          }}
        />
      </div>

      {/* Content */}
      <div className="relative z-10 flex flex-col h-full px-7 pt-5 pb-5">
        {/* Top bar */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <img src="/bloom.png" alt="" className="w-4 h-4 object-contain opacity-60" />
            <span className="text-[9px] font-bold tracking-[0.2em] uppercase text-white/25">Bloom</span>
          </div>
          <div className="flex items-center gap-1.5 bg-white/[0.03] border border-white/[0.05] rounded-full px-2 py-0.5">
            <span className="w-1.5 h-1.5 rounded-full bg-emerald-400 animate-pulse" />
            <span className="text-[8px] font-bold text-white/45 font-mono">{releaseInfo.version}</span>
          </div>
        </div>

        {/* Hero */}
        <div className="flex-1 flex flex-col items-center justify-center -mt-6">
          {/* Big icon — rotates on hover */}
          <div className="group cursor-default">
            <img
              src="/bloom.png"
              alt="Bloom"
              className="w-20 h-20 object-contain transition-transform duration-500 ease-out group-hover:rotate-180"
            />
          </div>

          {/* Title */}
          <div className="w-full mt-4 min-h-[60px]">
            <TextPressure
              text="BLOOM"
              flex
              alpha={false}
              stroke={false}
              width
              weight
              italic
              textColor="#ffffff"
              strokeColor="#5227FF"
              minFontSize={48}
              invertWeight
            />
          </div>
          <p
            className="text-[13px] text-white/30 mt-1 italic"
            style={{ fontFamily: 'var(--font-display)' }}
          >
            your windows, alive.
          </p>

          {/* Tagline */}
          <p className="text-[10px] text-white/25 mt-3 max-w-[280px] text-center leading-relaxed">
            A lightweight desktop shell that silences the Windows taskbar and adds a spring-animated notch, a macOS-style dock, and volume/brightness overlays — all under 10MB idle RAM.
          </p>
        </div>

        {/* Bottom actions */}
        <div className="flex gap-2.5 mt-auto">
          <a
            href={releaseInfo.downloadUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="flex-1 flex items-center justify-center gap-2 py-2.5 rounded-xl text-[11px] font-bold text-black transition-all hover:brightness-110 active:scale-[0.98] cursor-pointer"
            style={{
              background: accentColor,
              boxShadow: `0 8px 24px -6px ${accentColor}40`,
            }}
          >
            <Download size={12} strokeWidth={2.5} />
            Download Installer
          </a>
          <a
            href={githubUrl}
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center justify-center gap-2 py-2.5 px-5 rounded-xl bg-white/[0.05] hover:bg-white/[0.08] border border-white/[0.07] text-[11px] font-semibold text-white/70 transition-all active:scale-[0.98] cursor-pointer"
          >
            GitHub
            <ArrowUpRight size={10} />
          </a>
        </div>
      </div>
    </div>
  );
}


