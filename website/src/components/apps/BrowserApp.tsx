import { useState, useRef } from 'react';
import { ArrowLeft, ArrowRight, RotateCcw, ExternalLink, Lock, Globe } from 'lucide-react';

interface Tab {
  id: string;
  title: string;
  url: string;
}

function toEmbedUrl(url: string): string {
  const ytMatch = url.match(/(?:youtube\.com\/watch\?v=|youtu\.be\/)([a-zA-Z0-9_-]+)/);
  if (ytMatch) return `https://www.youtube.com/embed/${ytMatch[1]}`;
  if (url.includes('/embed/')) return url;
  return url;
}

function extractTitle(url: string): string {
  try {
    const host = new URL(url).hostname.replace('www.', '');
    return host.charAt(0).toUpperCase() + host.slice(1);
  } catch {
    return 'New Tab';
  }
}

const SIDEBAR_BOOKMARKS = [
  { name: 'Wikipedia', url: 'https://en.wikipedia.org' },
  { name: 'YouTube', url: 'https://www.youtube.com/embed/dQw4w9WgXcQ' },
  { name: 'RFC 2616', url: 'https://www.rfc-editor.org/rfc/rfc2616' },
  { name: 'Bloom GH', url: 'https://github.com/SehajveerSingh2005/bloom' },
];

export default function BrowserApp() {
  const [tabs, setTabs] = useState<Tab[]>([
    { id: '1', title: 'Wikipedia', url: 'https://en.wikipedia.org' },
  ]);
  const [activeTab, setActiveTab] = useState('1');
  const [inputValue, setInputValue] = useState('https://en.wikipedia.org');
  const [isLoading, setIsLoading] = useState(true);
  const [hasError, setHasError] = useState(false);
  const iframeRef = useRef<HTMLIFrameElement>(null);

  const navigate = (targetUrl: string) => {
    let finalUrl = targetUrl;
    if (!finalUrl.startsWith('http://') && !finalUrl.startsWith('https://')) {
      finalUrl = 'https://' + finalUrl;
    }
    const embedUrl = toEmbedUrl(finalUrl);
    setInputValue(finalUrl);
    setIsLoading(true);
    setHasError(false);
    setTabs((prev) =>
      prev.map((t) =>
        t.id === activeTab ? { ...t, url: embedUrl, title: extractTitle(finalUrl) } : t
      )
    );
  };

  const addTab = () => {
    const id = Date.now().toString();
    setTabs((prev) => [...prev, { id, title: 'New Tab', url: 'https://en.wikipedia.org' }]);
    setActiveTab(id);
    setInputValue('https://en.wikipedia.org');
    setIsLoading(true);
    setHasError(false);
  };

  const closeTab = (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (tabs.length === 1) return;
    setTabs((prev) => prev.filter((t) => t.id !== id));
    if (activeTab === id) {
      const remaining = tabs.filter((t) => t.id !== id);
      const next = remaining[remaining.length - 1];
      setActiveTab(next.id);
      setInputValue(next.url);
    }
  };

  const switchTab = (id: string) => {
    const tab = tabs.find((t) => t.id === id);
    if (tab) {
      setActiveTab(id);
      setInputValue(tab.url);
      setIsLoading(true);
      setHasError(false);
    }
  };

  const currentUrl = tabs.find((t) => t.id === activeTab)?.url || '';

  return (
    <div className="h-full w-full flex bg-black/20 select-none font-sans overflow-hidden">
      {/* Left sidebar */}
      <div className="w-[42px] shrink-0 flex flex-col items-center py-2 gap-1 bg-white/[0.02] border-r border-white/[0.05]">
        {/* Tabs */}
        {tabs.map((tab) => (
          <button
            key={tab.id}
            onClick={() => switchTab(tab.id)}
            className="relative group"
            title={tab.title}
          >
            <div
              className={`w-8 h-8 flex items-center justify-center rounded-lg transition-all ${
                activeTab === tab.id
                  ? 'bg-white/[0.08] text-white/70'
                  : 'text-white/25 hover:text-white/50 hover:bg-white/[0.04]'
              }`}
            >
              <Globe size={13} />
            </div>
            {activeTab === tab.id && (
              <div className="absolute left-0 top-1/2 -translate-y-1/2 w-[2px] h-3 rounded-r bg-white/50" />
            )}
            {tabs.length > 1 && (
              <span
                onClick={(e) => closeTab(tab.id, e)}
                className="absolute -top-0.5 -right-0.5 w-3 h-3 rounded-full bg-white/10 text-white/40 text-[7px] flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer"
              >
                x
              </span>
            )}
          </button>
        ))}

        <button
          onClick={addTab}
          className="w-8 h-8 flex items-center justify-center rounded-lg text-white/20 hover:text-white/40 hover:bg-white/[0.04] transition-all text-[14px]"
          title="New Tab"
        >
          +
        </button>

        <div className="w-5 h-px bg-white/[0.06] my-1" />

        {/* Bookmarks */}
        {SIDEBAR_BOOKMARKS.map((b) => (
          <button
            key={b.name}
            onClick={() => navigate(b.url)}
            className="w-8 h-8 flex items-center justify-center rounded-lg text-white/20 hover:text-white/50 hover:bg-white/[0.04] transition-all"
            title={b.name}
          >
            <Globe size={11} />
          </button>
        ))}
      </div>

      {/* Main area */}
      <div className="flex-1 flex flex-col min-w-0">
        {/* Toolbar */}
        <div className="shrink-0 flex items-center gap-1.5 px-3 py-1.5 bg-white/[0.02] border-b border-white/[0.05]">
          <NavBtn onClick={() => iframeRef.current?.contentWindow?.history.back()}>
            <ArrowLeft size={11} />
          </NavBtn>
          <NavBtn onClick={() => iframeRef.current?.contentWindow?.history.forward()}>
            <ArrowRight size={11} />
          </NavBtn>
          <NavBtn onClick={() => { setIsLoading(true); setHasError(false); iframeRef.current?.contentWindow?.location.reload(); }}>
            <RotateCcw size={10} />
          </NavBtn>

          <div className="flex-1 flex items-center gap-1.5 bg-white/[0.04] border border-white/[0.06] rounded-lg px-2.5 py-1 ml-1">
            <Lock size={9} className="text-white/25 shrink-0" />
            <input
              type="text"
              value={inputValue}
              onChange={(e) => setInputValue(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && navigate(inputValue)}
              className="flex-1 bg-transparent text-[10px] text-white/70 font-mono outline-none placeholder:text-white/20 min-w-0"
              placeholder="Enter URL..."
            />
            <button
              onClick={() => navigate(inputValue)}
              className="text-white/30 hover:text-white/60 transition-colors shrink-0"
            >
              <ExternalLink size={9} />
            </button>
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 relative bg-[#1a1a24]">
          {isLoading && !hasError && (
            <div className="absolute inset-0 flex items-center justify-center z-10">
              <div className="flex flex-col items-center gap-2">
                <div className="w-4 h-4 border-2 border-white/10 border-t-white/50 rounded-full animate-spin" />
                <span className="text-[9px] text-white/30 font-mono">{extractTitle(currentUrl)}</span>
              </div>
            </div>
          )}

          {hasError ? (
            <div className="absolute inset-0 flex items-center justify-center z-10">
              <div className="flex flex-col items-center gap-2 text-center px-8">
                <span className="text-[11px] text-white/50 font-semibold">Can't embed this site</span>
                <span className="text-[9px] text-white/25 leading-relaxed">
                  This site blocks iframe embedding.
                </span>
                <a
                  href={currentUrl}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="mt-1 px-3 py-1.5 rounded-lg bg-white/[0.06] border border-white/[0.08] text-[9px] font-semibold text-white/60 hover:bg-white/[0.1] transition-colors"
                >
                  Open in real browser
                </a>
              </div>
            </div>
          ) : (
            <iframe
              ref={iframeRef}
              src={currentUrl}
              className="w-full h-full border-0"
              onLoad={() => setIsLoading(false)}
              onError={() => { setIsLoading(false); setHasError(true); }}
              sandbox="allow-same-origin allow-scripts allow-popups allow-forms"
            />
          )}
        </div>
      </div>
    </div>
  );
}

function NavBtn({ children, onClick, disabled }: { children: React.ReactNode; onClick: () => void; disabled?: boolean }) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="w-6 h-6 flex items-center justify-center rounded-md text-white/30 hover:text-white/60 hover:bg-white/[0.06] disabled:opacity-20 disabled:cursor-not-allowed transition-all"
    >
      {children}
    </button>
  );
}
