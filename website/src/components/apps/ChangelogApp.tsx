import { useState, useEffect } from 'react';
import { ChevronDown, ChevronUp, Sparkles, Bug, Zap, Shield, Palette, Monitor, Terminal, ExternalLink } from 'lucide-react';

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

const getChangeIcon = (type: string) => {
  switch (type) {
    case 'feature': return <Sparkles size={10} className="text-emerald-400" />;
    case 'fix': return <Bug size={10} className="text-amber-400" />;
    case 'improvement': return <Zap size={10} className="text-blue-400" />;
    case 'security': return <Shield size={10} className="text-purple-400" />;
    case 'design': return <Palette size={10} className="text-pink-400" />;
    case 'performance': return <Monitor size={10} className="text-cyan-400" />;
    case 'removal': return <Bug size={10} className="text-red-400" />;
    default: return <Terminal size={10} className="text-white/40" />;
  }
};

const getChangeLabel = (type: string) => {
  switch (type) {
    case 'feature': return 'New';
    case 'fix': return 'Fix';
    case 'improvement': return 'Improved';
    case 'security': return 'Security';
    case 'design': return 'Design';
    case 'performance': return 'Perf';
    case 'removal': return 'Removed';
    default: return type.charAt(0).toUpperCase() + type.slice(1);
  }
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
      <div className="px-5 pt-5 pb-3 shrink-0">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <div className="w-5 h-5 rounded-md bg-white/10 flex items-center justify-center">
              <Terminal size={11} className="text-white/70" />
            </div>
            <span className="text-[11px] uppercase tracking-widest font-bold text-white/40">Changelog</span>
          </div>
          <a
            href={`https://github.com/${GITHUB_REPO}/releases`}
            target="_blank"
            rel="noopener noreferrer"
            className="flex items-center gap-1 text-[9px] text-white/30 hover:text-white/60 transition-colors"
          >
            View on GitHub <ExternalLink size={9} />
          </a>
        </div>
        <p className="text-[10px] text-white/30 mt-1">What's new in Bloom</p>
      </div>

      <div className="flex-1 overflow-y-auto px-5 pb-5">
        {loading && (
          <div className="flex items-center justify-center h-32">
            <div className="flex items-center gap-2 text-white/30">
              <div className="w-3 h-3 border-2 border-white/20 border-t-white/60 rounded-full animate-spin" />
              <span className="text-[11px]">Loading releases...</span>
            </div>
          </div>
        )}

        {error && (
          <div className="flex flex-col items-center justify-center h-32 gap-2">
            <span className="text-[11px] text-red-400/80">Failed to load releases</span>
            <span className="text-[9px] text-white/20">{error}</span>
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
              className="text-[9px] text-white/40 hover:text-white/60 underline transition-colors cursor-pointer"
            >
              Retry
            </button>
          </div>
        )}

        {!loading && !error && (
          <div className="relative">
            <div className="absolute left-[7px] top-0 bottom-0 w-[1px] bg-white/[0.06]" />

            <div className="space-y-1">
              {releases.map((release, idx) => {
                const isExpanded = expandedVersions.has(release.version);
                const isLatest = idx === 0 && !release.prerelease;
                const isMajor = isMajorVersion(release.version);

                return (
                  <div key={release.version} className="relative pl-6">
                    <div className={`absolute left-0 top-3 w-[15px] h-[15px] rounded-full border-2 flex items-center justify-center transition-all ${
                      isLatest
                        ? 'border-emerald-400 bg-emerald-400/20'
                        : isMajor
                        ? 'border-[#fa243c] bg-[#fa243c]/20'
                        : 'border-white/10 bg-white/5'
                    }`}>
                      <div className={`w-[5px] h-[5px] rounded-full ${
                        isLatest ? 'bg-emerald-400' : isMajor ? 'bg-[#fa243c]' : 'bg-white/20'
                      }`} />
                    </div>

                    <button
                      onClick={() => toggleVersion(release.version)}
                      className="w-full text-left py-2.5 group cursor-pointer"
                    >
                      <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2">
                          <span className={`text-[12px] font-bold ${
                            isLatest ? 'text-emerald-400' : isMajor ? 'text-[#fa243c]' : 'text-white/80'
                          }`}>
                            {release.version}
                          </span>
                          {isLatest && (
                            <span className="text-[7px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded bg-emerald-400/15 text-emerald-400">
                              Latest
                            </span>
                          )}
                          {isMajor && (
                            <span className="text-[7px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded bg-[#fa243c]/15 text-[#fa243c]">
                              Major
                            </span>
                          )}
                          {release.prerelease && (
                            <span className="text-[7px] font-bold uppercase tracking-wider px-1.5 py-0.5 rounded bg-amber-400/15 text-amber-400">
                              Pre-release
                            </span>
                          )}
                        </div>
                        <div className="flex items-center gap-2">
                          <span className="text-[9px] text-white/25 font-mono">{release.date}</span>
                          {isExpanded ? (
                            <ChevronUp size={12} className="text-white/30" />
                          ) : (
                            <ChevronDown size={12} className="text-white/30" />
                          )}
                        </div>
                      </div>
                    </button>

                    {isExpanded && (
                      <div className="pb-3 space-y-2 animate-in slide-in-from-top-1">
                        {release.sections.length > 0 ? (
                          release.sections.map((section, sIdx) => (
                            <div key={sIdx}>
                              {section.type !== 'note' && (
                                <div className="flex items-center gap-1.5 mb-1.5 ml-1">
                                  {getChangeIcon(section.type)}
                                  <span className="text-[8px] font-bold uppercase tracking-wider text-white/30">
                                    {getChangeLabel(section.type)}
                                  </span>
                                </div>
                              )}
                              <div className="space-y-1">
                                {section.items.map((item, iIdx) => (
                                  <div
                                    key={iIdx}
                                    className="flex items-start gap-2.5 py-1.5 px-2.5 rounded-lg bg-white/[0.02] border border-white/[0.03] hover:bg-white/[0.04] transition-colors"
                                  >
                                    {section.type === 'note' && (
                                      <div className="shrink-0 mt-0.5">
                                        <Terminal size={10} className="text-white/30" />
                                      </div>
                                    )}
                                    <span className="text-[10px] text-white/60 leading-relaxed">
                                      {item}
                                    </span>
                                  </div>
                                ))}
                              </div>
                            </div>
                          ))
                        ) : (
                          <div className="text-[10px] text-white/40 px-2.5 py-2 bg-white/[0.02] rounded-lg border border-white/[0.03] whitespace-pre-wrap leading-relaxed">
                            {release.body}
                          </div>
                        )}
                        <a
                          href={release.url}
                          target="_blank"
                          rel="noopener noreferrer"
                          className="inline-flex items-center gap-1 text-[9px] text-white/25 hover:text-white/50 transition-colors ml-2.5"
                        >
                          View release on GitHub <ExternalLink size={8} />
                        </a>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}