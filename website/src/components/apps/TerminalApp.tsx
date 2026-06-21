import { useState, useEffect, useRef, type FormEvent } from 'react';

interface TerminalAppProps {
  accentColor: string;
}

export default function TerminalApp({ accentColor }: TerminalAppProps) {
  const [history, setHistory] = useState<string[]>([
    "Initializing Bloom kernel v3.1.2...",
    "[INFO] Initializing Rust core module",
    "[INFO] Binding Windows DWM API listeners",
    "[INFO] Hooking taskbar window position listener",
    "[INFO] Registering IPC commands: load_pinned_apps, get_app_icon",
    "[INFO] Loading user settings from settings.json",
    "[INFO] Setup volume controller listener",
    "[INFO] Dock window initialized: transparent=true, width=1920",
    "[INFO] Top notch window initialized: x=0, y=0, height=360",
    "Bloom startup completed. Welcome!",
    "Type 'help' to see available commands."
  ]);
  const [input, setInput] = useState('');
  const terminalEndRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    if (terminalEndRef.current) {
      terminalEndRef.current.scrollIntoView({ behavior: 'smooth' });
    }
  }, [history]);

  const handleCommand = (cmd: string) => {
    const trimmed = cmd.trim().toLowerCase();
    let response: string[] = [];

    if (trimmed === 'help') {
      response = [
        "Available Commands:",
        "  help         Show this list of commands",
        "  features     List Bloom's major desktop features",
        "  systeminfo   Display mock system specs",
        "  about        Background story of Bloom's engineering",
        "  clear        Clear the screen"
      ];
    } else if (trimmed === 'features') {
      response = [
        "🌸 Bloom Features Outline:",
        "  - [Notch Module]: Expandable pill menu for calendar, weather & volume widget status.",
        "  - [Dock Module]: Pin apps, drag reordering, and hover thumbnail live window previews.",
        "  - [Audio Module]: Audio-reactive visualizer syncing with current music track beats.",
        "  - [Lightweight]: Tauri v2 window wrapper with native Rust overlay controls."
      ];
    } else if (trimmed === 'systeminfo') {
      response = [
        "💻 System Specifications:",
        "  OS:           Windows 11 Simulator",
        "  Shell:        Tauri v2 App Overlays",
        "  Backend:      Rust compiler v1.75+",
        "  Frontend:     React + TypeScript + TailwindCSS",
        "  RAM Footprint: 8.4 MB (active)"
      ];
    } else if (trimmed === 'about') {
      response = [
        "🛠️ Engineering Backstory:",
        "  Bloom was built as a solution to bulky, resource-heavy customization tools.",
        "  By replacing Electron with Tauri v2, the app compiles to a native Windows executable",
        "  that uses only raw system events and standard webview renders, cutting RAM usage by 95%."
      ];
    } else if (trimmed === 'clear') {
      setHistory([]);
      return;
    } else if (trimmed !== '') {
      response = [`bloom: command not found: '${cmd}'. Type 'help' for available actions.`];
    }

    setHistory((prev) => [...prev, `bloom@system:~$ ${cmd}`, ...response]);
  };

  const handleSubmit = (e: FormEvent) => {
    e.preventDefault();
    handleCommand(input);
    setInput('');
  };

  return (
    <div className="flex flex-col h-full bg-black/20 text-emerald-400 font-mono text-[12px] p-4 select-text selection:bg-white/10 selection:text-white">
      <div className="flex-1 overflow-y-auto space-y-1.5 scrollbar-thin">
        {history.map((line, idx) => (
          <div 
            key={idx} 
            className={`${
              line.startsWith('bloom@system') 
                ? 'text-white' 
                : line.startsWith('[INFO]') 
                  ? 'text-white/40' 
                  : line.startsWith('bloom: command not found') 
                    ? 'text-rose-400' 
                    : 'text-emerald-400'
            }`}
          >
            {line}
          </div>
        ))}
        <div ref={terminalEndRef} />
      </div>

      <form onSubmit={handleSubmit} className="flex items-center gap-1.5 border-t border-white/5 pt-2 mt-2 select-none">
        <span style={{ color: accentColor }}>bloom@system:~$</span>
        <input
          type="text"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          className="flex-1 bg-transparent text-white focus:outline-none caret-emerald-400 select-text"
          autoFocus
        />
      </form>
    </div>
  );
}
