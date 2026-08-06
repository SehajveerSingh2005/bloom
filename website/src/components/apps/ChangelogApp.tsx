import { useState, useEffect } from 'react';
import { ChevronDown, ChevronUp, Sparkles, Bug, Zap, Shield, Palette, Monitor, Terminal, ExternalLink, ArrowRight, Clock } from 'lucide-react';

const GITHUB_REPO = 'SehajveerSingh2005/bloom';

interface GitHubRelease {
  tag_name: string;
  name: string;
  published_at: string;
  body: string;
  html_url: string;
  prerelease: boolean;
}

interface ParsedRelease {
  version: string;
  name: string;
  date: string;
  url: string;
  prerelease: boolean;
  body: string;
  sections: { type: string; items: string[] }[];
}

function parseReleaseBody(body: string): { type: string; items: string[] }[] {
  if (!body) return [{ type: 'note', items: ['No release notes available.'] }];

  const sections: { type: string; items: string[] }[] = [];
  const lines = body.split('\n');
  let currentType = 'note';
  let currentItems: string[] = [];

  for (const line of lines) {
    const trimmed = line.trim();
    if (!trimmed) continue;

    const headerMatch = trimmed.match(/^#{1,4}\s+(.+)/);
    if (headerMatch) {
      if (currentItems.length > 0) {
        sections.push({ type: currentType, items: currentItems });
      }
      const headerText = headerMatch[1].toLowerCase();
      if (headerText.includes('added') || headerText.includes('new') || headerText.includes('feature')) {
        currentType = 'feature';
      } else if (headerText.includes('fix') || headerText.includes('bug')) {
        currentType = 'fix';
      } else if (headerText.includes('change') || headerText.includes('improv') || headerText.includes('update')) {
        currentType = 'improvement';
      } else if (headerText.includes('security')) {
        currentType = 'security';
      } else if (headerText.includes('design') || headerText.includes('ui') || headerText.includes('style')) {
        currentType = 'design';
      } else if (headerText.includes('performance') || headerText.includes('speed') || headerText.includes('optim')) {
        currentType = 'performance';
      } else if (headerText.includes('remove') || headerText.includes('deprecat')) {
        currentType = 'removal';
      } else {
        currentType = headerText;
      }
      currentItems = [];
      continue;
    }

    const listMatch = trimmed.match(/^[-*]\s+(.+)/) || trimmed.match(/^\d+\.\s+(.+)/);
    if (listMatch) {
      currentItems.push(listMatch[1]);
      continue;
    }

    if (currentItems.length > 0 && trimmed && !trimmed.startsWith('#')) {
      currentItems[currentItems.length - 1] += ' ' + trimmed;
    }
  }

  if (currentItems.length > 0) {
    sections.push({ type: currentType, items: currentItems });
  }

  if (sections.length === 0 && body.trim()) {
    sections.push({ type: 'note', items: [body.trim()] });
  }

  return sections;
}

function formatDate(dateStr: string): string {
  const d = new Date(dateStr);
  return d.toLocaleDateString('en-US', { month: 'short', day: 'numeric', year: 'numeric' });
}

function isMajorVersion(version: string): boolean {
  const minorMatch = version.replace(/^v/, '').match(/^\d+\.(\d+)/);
  return minorMatch ? minorMatch[1] === '0' : false;
}

const CHANGE_CONFIG: Record<string, { icon: React.ReactNode; label: string; color: string; bg: string }> = {
  feature:     { icon: <Sparkles size={13} />,   label: 'New',        color: 'text-emerald-400', bg: 'bg-emerald-400/10 border-emerald-400/20' },
  fix:         { icon: <Bug size={13} />,          label: 'Fix',        color: 'text-amber-400',   bg: 'bg-amber-400/10 border-amber-400/20' },
  improvement: { icon: <Zap size={13} />,          label: 'Improved',   color: 'text-blue-400',    bg: 'bg-blue-400/10 border-blue-400/20' },
  security:    { icon: <Shield size={13} />,       label: 'Security',   color: 'text-purple-400',  bg: 'bg-purple-400/10 border-purple-400/20' },
  design:      { icon: <Palette size={13} />,      label: 'Design',     color: 'text-pink-400',    bg: 'bg-pink-400/10 border-pink-400/20' },
  performance: { icon: <Monitor size={13} />,      label: 'Perf',       color: 'text-cyan-400',    bg: 'bg-cyan-400/10 border-cyan-400/20' },
  removal:     { icon: <Bug size={13} />,          label: 'Removed',    color: 'text-red-400',     bg: 'bg-red-400/10 border-red-400/20' },
  note:        { icon: <Terminal size={13} />,     label: 'Note',       color: 'text-white/50',    bg: 'bg-white/5 border-white/10' },
};

export default function ChangelogApp() {
  const [releases, setReleases] = useState<ParsedRelease[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [expandedVersions, setExpandedVersions] = useState<Set<string>>(new Set());

  useEffect(() => {
    fetch(`https://api.github.com/repos/${GITHUB_REPO}/releases?per_page=20`)
      .then((res) => {
        if (!res.ok) throw new Error(`GitHub API ${res.status}`);
        return res.json();
      })
      .then((data: GitHubRelease[]) => {
        const parsed: ParsedRelease[] = data.map((r) => ({
          version: r.tag_name,
          name: r.name || r.tag_name,
          date: formatDate(r.published_at),
          url: r.html_url,
          prerelease: r.prerelease,
          body: r.body || '',
          sections: parseReleaseBody(r.body || ''),
        }));
        setReleases(parsed);
        if (parsed.length > 0) {
          setExpandedVersions(new Set([parsed[0].version]));
        }
        setLoading(false);
      })
      .catch((err) => {
        setError(err.message);
        setLoading(false);
      });
  }, []);

  const toggleVersion = (version: string) => {
    setExpandedVersions((prev) => {
      const next = new Set(prev);
      if (next.has(version)) {
        next.delete(version);
      } else {
        next.add(version);
      }
      return next;
    });
  };

  return (
    <div className="flex flex-col h-full w-full bg-transparent text-white select-none overflow-hidden font-sans">
      {/* Header */}
      <div className="px-6 pt-6 pb-4 shrink-0">
        <div className="flex items-center justify-between mb-1">
          <h1 className="text-[22px] font-bold tracking-tight text-white/90">Changelog</h1>
          <a
            href={`https://github.com/${GITHUB_REPO}/releases`}
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1.5 text-[11px] text-white/30 hover:text-white/60 transition-colors"
          >
            GitHub <ExternalLink size={10} />
          </a>
        </div>
        <p className="text-[12px] text-white/35">What's new in Bloom</p>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-y-auto px-6 pb-6 scrollbar-thin">
        {loading && (
          <div className="flex items-center justify-center h-40">
            <div className="flex flex-col items-center gap-3">
              <div className="w-5 h-5 border-2 border-white/15 border-t-white/60 rounded-full animate-spin" />
              <span className="text-[12px] text-white/35">Loading releases...</span>
            </div>
          </div>
        )}

        {error && (
          <div className="flex flex-col items-center justify-center h-40 gap-3">
            <span className="text-[13px] text-red-400/80 font-medium">Failed to load releases</span>
            <span className="text-[11px] text-white/25">{error}</span>
            <button
              onClick={() => {
                setLoading(true);
                setError(null);
                fetch(`https://api.github.com/repos/${GITHUB_REPO}/releases?per_page=20`)
                  .then((r) => r.json())
                  .then((data: GitHubRelease[]) => {
                    setReleases(data.map((r) => ({
                      version: r.tag_name,
                      name: r.name || r.tag_name,
                      date: formatDate(r.published_at),
                      url: r.html_url,
                      prerelease: r.prerelease,
                      body: r.body || '',
                      sections: parseReleaseBody(r.body || ''),
                    })));
                    setLoading(false);
                  })
                  .catch((e) => { setError(e.message); setLoading(false); });
              }}
              className="text-[11px] text-white/40 hover:text-white/60 underline transition-colors cursor-pointer"
            >
              Retry
            </button>
          </div>
        )}

        {!loading && !error && (
          <div className="space-y-2">
            {releases.map((release, idx) => {
              const isExpanded = expandedVersions.has(release.version);
              const isLatest = idx === 0 && !release.prerelease;
              const isMajor = isMajorVersion(release.version);

              return (
                <div key={release.version}>
                  {/* Latest release hero */}
                  {isLatest && isExpanded && (
                    <div className="mb-4 pb-4 border-b border-white/[0.06]">
                      <div className="flex items-center gap-2 mb-3">
                        <span className="text-[10px] font-bold uppercase tracking-widest text-emerald-400 bg-emerald-400/10 border border-emerald-400/20 px-2 py-0.5 rounded-full">
                          Latest Release
                        </span>
                        <span className="text-[11px] text-white/25 font-mono">{release.date}</span>
                      </div>
                      <h2 className="text-[32px] font-extrabold tracking-tight text-white/95 leading-none mb-1">
                        {release.version}
                      </h2>
                      {release.name !== release.version && (
                        <p className="text-[13px] text-white/40 mt-1">{release.name}</p>
                      )}
                      {/* Section summary chips */}
                      {release.sections.length > 0 && (
                        <div className="flex flex-wrap gap-1.5 mt-3">
                          {release.sections.map((s, i) => {
                            const cfg = CHANGE_CONFIG[s.type] || CHANGE_CONFIG.note;
                            return (
                              <span
                                key={i}
                                className={`inline-flex items-center gap-1 text-[10px] font-semibold px-2 py-0.5 rounded-full border ${cfg.color} ${cfg.bg}`}
                              >
                                {cfg.icon}
                                {cfg.label}
                                <span className="opacity-50">{s.items.length}</span>
                              </span>
                            );
                          })}
                        </div>
                      )}
                    </div>
                  )}

                  {/* Release card */}
                  <div className={`group rounded-xl transition-all ${
                    isLatest ? 'bg-white/[0.03] border border-white/[0.06]' : ''
                  }`}>
                    <button
                      onClick={() => toggleVersion(release.version)}
                      className="w-full text-left px-4 py-3 cursor-pointer"
                    >
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-3">
                          {/* Timeline dot */}
                          <div className={`relative shrink-0 w-3 h-3 rounded-full ${
                            isLatest
                              ? 'bg-emerald-400 shadow-[0_0_8px_rgba(52,211,153,0.4)]'
                              : isMajor
                              ? 'bg-[#fa243c] shadow-[0_0_8px_rgba(250,36,60,0.3)]'
                              : 'bg-white/20'
                          }`}>
                            {isLatest && (
                              <div className="absolute inset-0 rounded-full bg-emerald-400/30 animate-ping" />
                            )}
                          </div>
                          <div>
                            <div className="flex items-center gap-2">
                              <span className={`text-[15px] font-bold ${
                                isLatest ? 'text-emerald-400' : isMajor ? 'text-[#fa243c]' : 'text-white/80'
                              }`}>
                                {release.version}
                              </span>
                              {isMajor && !isLatest && (
                                <span className="text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-[#fa243c]/10 text-[#fa243c] border border-[#fa243c]/20">
                                  Major
                                </span>
                              )}
                              {release.prerelease && (
                                <span className="text-[9px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded-full bg-amber-400/10 text-amber-400 border border-amber-400/20">
                                  Pre
                                </span>
                              )}
                            </div>
                            {!isLatest && release.name !== release.version && (
                              <p className="text-[11px] text-white/30 mt-0.5">{release.name}</p>
                            )}
                          </div>
                        </div>
                        <div className="flex items-center gap-2">
                          <Clock size={10} className="text-white/20" />
                          <span className="text-[11px] text-white/30 font-mono">{release.date}</span>
                          {isExpanded ? (
                            <ChevronUp size={14} className="text-white/30" />
                          ) : (
                            <ChevronDown size={14} className="text-white/25" />
                          )}
                        </div>
                      </div>
                    </button>

                    {/* Expanded content */}
                    {isExpanded && (
                      <div className="px-4 pb-4 space-y-3">
                        {release.sections.length > 0 ? (
                          release.sections.map((section, sIdx) => {
                            const cfg = CHANGE_CONFIG[section.type] || CHANGE_CONFIG.note;
                            return (
                              <div key={sIdx}>
                                <div className="flex items-center gap-2 mb-2">
                                  <span className={cfg.color}>{cfg.icon}</span>
                                  <span className={`text-[11px] font-bold uppercase tracking-wider ${cfg.color}`}>
                                    {cfg.label}
                                  </span>
                                  <span className="text-[10px] text-white/20">{section.items.length}</span>
                                </div>
                                <div className="space-y-1.5">
                                  {section.items.map((item, iIdx) => (
                                    <div
                                      key={iIdx}
                                      className="flex items-center gap-3 py-2 px-3 rounded-lg bg-white/[0.02] border border-white/[0.04] hover:bg-white/[0.05] transition-colors"
                                    >
                                      <div className={`shrink-0 w-1.5 h-1.5 rounded-full ${cfg.color.replace('text-', 'bg-')}`} />
                                      <span className="text-[12px] text-white/60 leading-relaxed">
                                        {item}
                                      </span>
                                    </div>
                                  ))}
                                </div>
                              </div>
                            );
                          })
                        ) : (
                          <div className="text-[12px] text-white/40 px-3 py-3 bg-white/[0.02] rounded-lg border border-white/[0.04] whitespace-pre-wrap leading-relaxed">
                            {release.body}
                          </div>
                        )}
                        <a
                          href={release.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1.5 text-[11px] text-white/25 hover:text-white/50 transition-colors group/link"
                        >
                          View on GitHub
                          <ArrowRight size={10} className="group-hover/link:translate-x-0.5 transition-transform" />
                        </a>
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
