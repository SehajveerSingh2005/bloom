import { useState, useEffect, useRef } from 'react';
import { Cpu, HardDrive, Activity, Layers } from 'lucide-react';

function useSimulatedValue(base: number, variance: number, interval = 1500) {
  const [value, setValue] = useState(base);
  useEffect(() => {
    const id = setInterval(() => {
      setValue(base + (Math.random() - 0.5) * variance);
    }, interval);
    return () => clearInterval(id);
  }, [base, variance, interval]);
  return value;
}

function Bar({ label, value, max, color }: { label: string; value: number; max: number; color: string }) {
  const pct = Math.min((value / max) * 100, 100);
  return (
    <div className="flex flex-col gap-1">
      <div className="flex justify-between items-center">
        <span className="text-[9px] font-semibold text-white/40 uppercase tracking-wider">{label}</span>
        <span className="text-[10px] font-bold text-white/70 font-mono">{Math.round(value)}%</span>
      </div>
      <div className="h-1.5 bg-white/[0.06] rounded-full overflow-hidden">
        <div
          className="h-full rounded-full transition-all duration-700 ease-out"
          style={{ width: `${pct}%`, background: color }}
        />
      </div>
    </div>
  );
}

function MiniGraph({ data, color }: { data: number[]; color: string }) {
  const max = Math.max(...data, 1);
  const h = 28;
  const w = 100;
  const points = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (v / max) * h;
    return `${x},${y}`;
  }).join(' ');

  return (
    <svg width={w} height={h} className="opacity-60">
      <defs>
        <linearGradient id={`grad-${color.replace('#', '')}`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0%" stopColor={color} stopOpacity="0.3" />
          <stop offset="100%" stopColor={color} stopOpacity="0" />
        </linearGradient>
      </defs>
      <polyline
        fill="none"
        stroke={color}
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        points={points}
      />
      <polygon
        fill={`url(#grad-${color.replace('#', '')})`}
        points={`0,${h} ${points} ${w},${h}`}
      />
    </svg>
  );
}

export default function PerformanceMonitor() {
  const cpu = useSimulatedValue(12, 8, 1200);
  const ram = useSimulatedValue(8.4, 2, 2000);
  const disk = useSimulatedValue(34, 4, 3000);
  const gpu = useSimulatedValue(5, 6, 1800);

  const [cpuHistory, setCpuHistory] = useState(() => Array(20).fill(0).map(() => 8 + Math.random() * 8));
  const [ramHistory, setRamHistory] = useState(() => Array(20).fill(0).map(() => 7 + Math.random() * 3));

  useEffect(() => {
    const id = setInterval(() => {
      setCpuHistory((prev) => [...prev.slice(1), cpu]);
      setRamHistory((prev) => [...prev.slice(1), ram]);
    }, 1200);
    return () => clearInterval(id);
  }, [cpu, ram]);

  const processes = [
    { name: 'bloom.exe', cpu: 1.2, ram: 4.1, color: '#34d399' },
    { name: 'bloom-daemon', cpu: 0.3, ram: 1.8, color: '#60a5fa' },
    { name: 'bloom-notch', cpu: 0.1, ram: 1.2, color: '#a78bfa' },
  ];

  return (
    <div className="h-full w-full bg-transparent text-white flex flex-col select-none overflow-hidden font-sans">
      {/* Header */}
      <div className="px-5 pt-4 pb-3 shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-5 h-5 rounded-md bg-emerald-500/15 flex items-center justify-center">
            <Activity size={11} className="text-emerald-400" />
          </div>
          <span className="text-[11px] uppercase tracking-widest font-bold text-white/40">Performance</span>
        </div>
      </div>

      <div className="flex-1 overflow-y-auto px-5 pb-5 space-y-4">
        {/* Live bars */}
        <div className="space-y-3">
          <Bar label="CPU" value={cpu} max={100} color="#34d399" />
          <Bar label="Memory" value={(ram / 16) * 100} max={100} color="#60a5fa" />
          <Bar label="Disk" value={disk} max={100} color="#a78bfa" />
          <Bar label="GPU" value={gpu} max={100} color="#f472b6" />
        </div>

        {/* Mini graphs */}
        <div className="grid grid-cols-2 gap-3">
          <div className="bg-white/[0.03] border border-white/[0.05] rounded-xl p-3">
            <div className="flex items-center gap-1.5 mb-2">
              <Cpu size={10} className="text-emerald-400" />
              <span className="text-[8px] font-bold uppercase tracking-wider text-white/30">CPU History</span>
            </div>
            <MiniGraph data={cpuHistory} color="#34d399" />
          </div>
          <div className="bg-white/[0.03] border border-white/[0.05] rounded-xl p-3">
            <div className="flex items-center gap-1.5 mb-2">
              <HardDrive size={10} className="text-blue-400" />
              <span className="text-[8px] font-bold uppercase tracking-wider text-white/30">RAM History</span>
            </div>
            <MiniGraph data={ramHistory} color="#60a5fa" />
          </div>
        </div>

        {/* System stats */}
        <div className="grid grid-cols-3 gap-2">
          <StatPill label="RAM" value={`${ram.toFixed(1)}MB`} sub="/ 16GB" />
          <StatPill label="Uptime" value="4h 23m" sub="since boot" />
          <StatPill label="FPS" value="120" sub="rendering" />
        </div>

        {/* Process list */}
        <div>
          <div className="flex items-center gap-1.5 mb-2">
            <Layers size={10} className="text-white/30" />
            <span className="text-[8px] font-bold uppercase tracking-wider text-white/30">Active Processes</span>
          </div>
          <div className="space-y-1">
            {processes.map((p) => (
              <div
                key={p.name}
                className="flex items-center justify-between py-1.5 px-2.5 rounded-lg bg-white/[0.02] border border-white/[0.03]"
              >
                <div className="flex items-center gap-2">
                  <div className="w-1.5 h-1.5 rounded-full" style={{ background: p.color }} />
                  <span className="text-[10px] font-mono text-white/60">{p.name}</span>
                </div>
                <div className="flex gap-4">
                  <span className="text-[9px] font-mono text-white/30">{p.cpu}%</span>
                  <span className="text-[9px] font-mono text-white/30">{p.ram}MB</span>
                </div>
              </div>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

function StatPill({ label, value, sub }: { label: string; value: string; sub: string }) {
  return (
    <div className="bg-white/[0.03] border border-white/[0.05] rounded-xl p-2.5 text-center">
      <span className="text-[7px] font-bold uppercase tracking-wider text-white/25 block">{label}</span>
      <span className="text-[14px] font-black text-white/80 block mt-0.5">{value}</span>
      <span className="text-[7px] text-white/20 block">{sub}</span>
    </div>
  );
}
