import { useRef, useEffect, useState } from 'react';
import {
  Home,
  Compass,
  Radio,
  Volume1,
  Volume2,
  Play,
  Pause,
  SkipForward,
  SkipBack,
  ListMusic,
  Share2,
  Music
} from 'lucide-react';
import godsPlanMp3 from '../../assets/gods-plan.mp3';
import godsPlanJpg from '../../assets/gods-plan.jpg';

interface MusicAppProps {
  playback: {
    isPlaying: boolean;
    trackTitle: string;
    trackArtist: string;
    currentTime: number;
    duration: number;
    volume: number;
    trackIndex?: number;
    trackCover?: string;
    tracksCount?: number;
  };
  setPlaybackState: (state: Partial<MusicAppProps['playback']>) => void;
  setVisualizerData: (frequencies: number[]) => void;
}

// Inline 5-bar visualizer matching Bloom's design style
const Visualizer = ({ isPlaying, frequencies }: { isPlaying: boolean; frequencies: number[] }) => {
  return (
    <div className="flex items-center justify-center gap-[2px] h-[12px] w-[20px] shrink-0">
      {frequencies.map((value, i) => (
        <div
          key={i}
          className="w-[2.5px] bg-[#fa243c] rounded-[1px] transition-all duration-150 origin-center"
          style={{
            height: isPlaying ? `${Math.max(20, value * 100)}%` : '20%',
            opacity: isPlaying ? 0.95 : 0.4,
          }}
        />
      ))}
    </div>
  );
};

export default function MusicApp({
  playback,
  setPlaybackState,
  setVisualizerData,
}: MusicAppProps) {
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const analyserRef = useRef<AnalyserNode | null>(null);
  const sourceNodeRef = useRef<MediaElementAudioSourceNode | null>(null);
  const animationFrameRef = useRef<number | null>(null);

  const [localVisualizerData, setLocalVisualizerData] = useState<number[]>([0.18, 0.18, 0.18, 0.18, 0.18]);

  // Single tracks list
  const [tracks] = useState([
    {
      title: "God's Plan",
      artist: "Drake",
      album: "Scorpion",
      url: godsPlanMp3,
      cover: godsPlanJpg,
      explicit: true,
      durationStr: "3:18"
    },
    {
      title: "Golden Hour Bloom",
      artist: "Aesthetic Lo-Fi",
      album: "Chill Overlays Vol. 1",
      url: "https://www.soundhelix.com/examples/mp3/SoundHelix-Song-1.mp3",
      cover: "https://images.unsplash.com/photo-1518609878373-06d740f60d8b?w=300&auto=format&fit=crop&q=60",
      explicit: false,
      durationStr: "6:12"
    },
    {
      title: "Rust & Compile",
      artist: "Tauri Beats",
      album: "Zero Footprint",
      url: "https://www.soundhelix.com/examples/mp3/SoundHelix-Song-2.mp3",
      cover: "https://images.unsplash.com/photo-1614680376593-902f74fa0d41?w=300&auto=format&fit=crop&q=60",
      explicit: true,
      durationStr: "7:05"
    }
  ]);

  const [currentTrackIndex, setCurrentTrackIndex] = useState(0); // Default to God's Plan
  const track = tracks[currentTrackIndex];

  // Sync current track index from parent state (Notch controls)
  useEffect(() => {
    if (playback.trackIndex !== undefined && playback.trackIndex !== currentTrackIndex && playback.trackIndex < tracks.length) {
      setCurrentTrackIndex(playback.trackIndex);
    }
  }, [playback.trackIndex, tracks.length]);

  // Sync track details to parent state on change
  useEffect(() => {
    setPlaybackState({
      trackTitle: track.title,
      trackArtist: track.artist,
      trackCover: track.cover,
      trackIndex: currentTrackIndex,
      tracksCount: tracks.length,
    });
  }, [currentTrackIndex, tracks.length]);

  // Initialize audio element ONCE
  useEffect(() => {
    const audio = new Audio();
    audioRef.current = audio;
    audio.volume = playback.volume;

    const handleLoadedMetadata = () => {
      setPlaybackState({ duration: audio.duration });
    };

    const handleTimeUpdate = () => {
      setPlaybackState({ currentTime: audio.currentTime });
    };

    const handleEnded = () => {
      handleNext();
    };

    audio.addEventListener('loadedmetadata', handleLoadedMetadata);
    audio.addEventListener('timeupdate', handleTimeUpdate);
    audio.addEventListener('ended', handleEnded);

    return () => {
      audio.pause();
      audio.removeEventListener('loadedmetadata', handleLoadedMetadata);
      audio.removeEventListener('timeupdate', handleTimeUpdate);
      audio.removeEventListener('ended', handleEnded);
      if (audioContextRef.current) {
        audioContextRef.current.close().catch(() => { });
      }
      if (animationFrameRef.current) {
        cancelAnimationFrame(animationFrameRef.current);
      }
    };
  }, []);

  // Update track source when index changes
  useEffect(() => {
    if (!audioRef.current) return;

    audioRef.current.pause();
    audioRef.current.src = track.url;
    audioRef.current.load();

    if (playback.isPlaying) {
      audioRef.current.play().then(() => {
        startVisualizerLoop();
      }).catch(e => {
        console.warn("Play failed on track change:", e);
      });
    }
  }, [currentTrackIndex]);

  // Synchronize audio play/pause state with parent
  useEffect(() => {
    if (!audioRef.current) return;

    if (playback.isPlaying) {
      if (!audioContextRef.current) {
        setupWebAudio();
      }

      audioRef.current.play().then(() => {
        startVisualizerLoop();
      }).catch(e => {
        console.warn("Autoplay blocked or failed:", e);
        setPlaybackState({ isPlaying: false });
      });
    } else {
      audioRef.current.pause();
    }
  }, [playback.isPlaying]);

  // Sync volume changes from parent state
  useEffect(() => {
    if (audioRef.current) {
      audioRef.current.volume = playback.volume;
    }
  }, [playback.volume]);

  // Clear visualizer data when paused
  useEffect(() => {
    if (!playback.isPlaying) {
      setVisualizerData([0.18, 0.18, 0.18, 0.18, 0.18]);
      setLocalVisualizerData([0.18, 0.18, 0.18, 0.18, 0.18]);
    }
  }, [playback.isPlaying]);

  // Setup Web Audio Analyser
  const setupWebAudio = () => {
    if (!audioRef.current) return;
    try {
      const AudioContextClass = window.AudioContext || (window as any).webkitAudioContext;
      const ctx = new AudioContextClass();
      audioContextRef.current = ctx;

      const analyser = ctx.createAnalyser();
      analyser.fftSize = 64;
      analyserRef.current = analyser;

      const source = ctx.createMediaElementSource(audioRef.current);
      sourceNodeRef.current = source;

      source.connect(analyser);
      analyser.connect(ctx.destination);
    } catch (err) {
      console.warn("Failed to create Web Audio graph:", err);
    }
  };

  // Visualizer tick loop
  const startVisualizerLoop = () => {
    if (animationFrameRef.current) cancelAnimationFrame(animationFrameRef.current);

    const tick = () => {
      if (playback.isPlaying && analyserRef.current) {
        const bufferLength = analyserRef.current.frequencyBinCount;
        const dataArray = new Uint8Array(bufferLength);
        analyserRef.current.getByteFrequencyData(dataArray);

        const normalized = Array.from(dataArray)
          .slice(0, 5)
          .map((v) => Math.max(0.18, v / 255));
        setVisualizerData(normalized);
        setLocalVisualizerData(normalized);
      } else if (playback.isPlaying) {
        const mockFrequencies = Array.from({ length: 5 }, () =>
          0.2 + Math.random() * 0.7
        );
        setVisualizerData(mockFrequencies);
        setLocalVisualizerData(mockFrequencies);
      }

      animationFrameRef.current = requestAnimationFrame(tick);
    };

    tick();
  };

  const handleNext = () => {
    const nextIdx = (currentTrackIndex + 1) % tracks.length;
    setPlaybackState({ trackIndex: nextIdx, isPlaying: true, currentTime: 0 });
    setCurrentTrackIndex(nextIdx);
  };

  const handlePrev = () => {
    const prevIdx = (currentTrackIndex - 1 + tracks.length) % tracks.length;
    setPlaybackState({ trackIndex: prevIdx, isPlaying: true, currentTime: 0 });
    setCurrentTrackIndex(prevIdx);
  };

  const togglePlay = () => {
    setPlaybackState({ isPlaying: !playback.isPlaying });
  };

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    const time = parseFloat(e.target.value);
    if (audioRef.current) {
      audioRef.current.currentTime = time;
      setPlaybackState({ currentTime: time });
    }
  };

  const handleVolumeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const vol = parseFloat(e.target.value);
    if (audioRef.current) {
      audioRef.current.volume = vol;
    }
    setPlaybackState({ volume: vol });
  };

  const selectTrack = (index: number) => {
    setPlaybackState({ trackIndex: index, isPlaying: true, currentTime: 0 });
    setCurrentTrackIndex(index);
  };

  const formatTime = (secs: number) => {
    if (isNaN(secs)) return "0:00";
    const minutes = Math.floor(secs / 60);
    const seconds = Math.floor(secs % 60);
    return `${minutes}:${seconds < 10 ? '0' : ''}${seconds}`;
  };

  return (
    <div className="flex flex-col h-full w-full bg-transparent text-white font-sans overflow-hidden select-none relative">
      {/* Blurred Album Art Aura Bleed */}
      <div className="absolute inset-0 pointer-events-none overflow-hidden z-0 transition-all duration-700 ease-in-out">
        <div
          className="absolute inset-[-40px] bg-cover bg-center filter blur-[60px] opacity-[0.12]"
          style={{ backgroundImage: `url(${track.cover})` }}
        />
      </div>

      {/* 1. TOP PLAYER BAR (Authentic Apple Music Desktop Style) */}
      <div className="h-[52px] bg-black/20 border-b border-white/[0.06] flex items-center justify-between px-4 z-10 shrink-0 select-none">

        {/* Left Side: Playback Controls + Volume Slider */}
        <div className="flex items-center gap-4">
          {/* Controls Group */}
          <div className="flex items-center gap-3">
            <button
              onClick={handlePrev}
              className="text-white/75 hover:text-white transition-colors active:scale-95 cursor-pointer"
              title="Previous"
            >
              <SkipBack size={15} fill="currentColor" stroke="none" />
            </button>
            <button
              onClick={togglePlay}
              className="w-7 h-7 rounded-full bg-white/10 hover:bg-white/15 text-white flex items-center justify-center active:scale-90 transition-all cursor-pointer"
              title={playback.isPlaying ? "Pause" : "Play"}
            >
              {playback.isPlaying ? (
                <Pause size={12} fill="currentColor" stroke="none" />
              ) : (
                <Play size={12} fill="currentColor" stroke="none" className="translate-x-[0.5px]" />
              )}
            </button>
            <button
              onClick={handleNext}
              className="text-white/75 hover:text-white transition-colors active:scale-95 cursor-pointer"
              title="Next"
            >
              <SkipForward size={15} fill="currentColor" stroke="none" />
            </button>
          </div>

          {/* Volume Control */}
          <div className="flex items-center gap-1.5 w-[90px]">
            <span className="text-white/40">
              <Volume1 size={11} />
            </span>
            <input
              type="range"
              min="0"
              max="1"
              step="0.01"
              value={playback.volume}
              onChange={handleVolumeChange}
              className="w-full h-[2.5px] bg-white/10 rounded-lg appearance-none cursor-pointer accent-[#fa243c]"
              style={{
                background: `linear-gradient(to right, #fa243c 0%, #fa243c ${playback.volume * 100
                  }%, rgba(255,255,255,0.1) ${playback.volume * 100}%, rgba(255,255,255,0.1) 100%)`
              }}
            />
            <span className="text-white/40">
              <Volume2 size={11} />
            </span>
          </div>
        </div>

        {/* Center: LCD Now Playing Display Capsule */}
        <div className="relative w-[300px] h-[36px] bg-black/40 border border-white/[0.06] rounded-md px-2.5 flex items-center justify-between overflow-hidden group">
          <div className="flex items-center gap-2 min-w-0 flex-1">
            <img src={track.cover} alt="Cover art" className="w-[22px] h-[22px] rounded-[3px] object-cover shrink-0" />
            <div className="flex flex-col min-w-0 leading-tight">
              <div className="flex items-center gap-1">
                <span className="text-[10px] font-semibold text-white/90 truncate">{track.title}</span>
                {track.explicit && (
                  <span className="bg-white/15 text-white/60 text-[7px] font-bold px-0.75 py-0.25 rounded shrink-0">E</span>
                )}
              </div>
              <span className="text-[8.5px] text-white/40 truncate">{track.artist}</span>
            </div>
          </div>

          {/* Inline visualizer & Remaining Time display inside LCD */}
          <div className="flex items-center gap-2.5 ml-2">
            <Visualizer isPlaying={playback.isPlaying} frequencies={localVisualizerData} />
            <span className="text-[8.5px] text-white/35 font-mono select-none">
              -{formatTime(playback.duration - playback.currentTime)}
            </span>
          </div>

          {/* Integrated thin Progress Scrubber along the bottom */}
          <input
            type="range"
            min="0"
            max={playback.duration || 100}
            value={playback.currentTime}
            onChange={handleSeek}
            className="absolute bottom-0 left-0 right-0 w-full h-[1.5px] bg-transparent appearance-none cursor-pointer accent-[#fa243c]"
            style={{
              background: `linear-gradient(to right, #fa243c 0%, #fa243c ${playback.duration ? (playback.currentTime / playback.duration) * 100 : 0
                }%, rgba(255,255,255,0.06) ${playback.duration ? (playback.currentTime / playback.duration) * 100 : 0
                }%, rgba(255,255,255,0.06) 100%)`
            }}
          />
        </div>

        {/* Right Side: Airplay/Lyrics & List toggles */}
        <div className="flex items-center gap-3 text-white/50">
          <button className="hover:text-white transition-colors cursor-pointer">
            <Share2 size={13} />
          </button>
          <button className="hover:text-white transition-colors cursor-pointer">
            <ListMusic size={13} />
          </button>
        </div>

      </div>

      {/* 2. MAIN LAYOUT */}
      <div className="flex flex-row flex-1 overflow-hidden z-10 bg-transparent">

        {/* Left Sidebar */}
        <div className="w-[145px] bg-black/15 border-r border-white/[0.04] p-3 flex flex-col justify-between text-[11px] shrink-0">
          <div>
            <div className="flex items-center gap-1.5 px-2 mb-4 text-[#fa243c] select-none font-bold">
              <Music size={13} className="fill-current" />
              <span>Library</span>
            </div>

            {/* Nav Links */}
            <div className="space-y-0.5">
              <div className="flex items-center gap-2 px-2 py-1.5 rounded bg-[#fa243c]/15 text-[#fa243c] font-semibold cursor-pointer">
                <Home size={12} strokeWidth={2.2} /> Listen Now
              </div>
              <div className="flex items-center gap-2 px-2 py-1.5 rounded text-white/70 hover:bg-white/[0.04] hover:text-white cursor-pointer transition-all">
                <Compass size={12} strokeWidth={2.2} /> Browse
              </div>
              <div className="flex items-center gap-2 px-2 py-1.5 rounded text-white/70 hover:bg-white/[0.04] hover:text-white cursor-pointer transition-all">
                <Radio size={12} strokeWidth={2.2} /> Radio
              </div>
            </div>

            {/* Playlist Labels */}
            <div className="mt-4">
              <div className="px-2 text-[8px] font-bold text-white/35 uppercase tracking-widest mb-1.5">My Music</div>
              <div className="space-y-0.5">
                <div className="px-2 py-1.5 rounded text-white/70 hover:bg-white/[0.04] hover:text-white cursor-pointer transition-all truncate">Recently Played</div>
                <div className="px-2 py-1.5 rounded text-white/70 hover:bg-white/[0.04] hover:text-white cursor-pointer transition-all truncate">Albums</div>
                <div className="px-2 py-1.5 rounded text-white/70 hover:bg-white/[0.04] hover:text-white cursor-pointer transition-all truncate">Songs</div>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-2 px-1 border-t border-white/[0.05] pt-2">
            <div className="w-4.5 h-4.5 rounded-full bg-[#fa243c] flex items-center justify-center font-bold text-[8px] text-white">
              ZS
            </div>
            <span className="font-semibold text-white/80 text-[9px] truncate">zesty singh</span>
          </div>
        </div>

        {/* Right Main Details Panel */}
        <div className="flex-1 overflow-y-auto p-5 select-none bg-gradient-to-b from-[#fa243c]/5 to-transparent">

          {/* Header Album Banner Info */}
          <div className="flex gap-4 mb-6">
            <div className="w-[85px] h-[85px] rounded-md overflow-hidden border border-white/10 shrink-0 shadow-lg">
              <img src={track.cover} alt="album art" className="w-full h-full object-cover" />
            </div>
            <div className="flex flex-col justify-end">
              <span className="text-[9px] font-bold tracking-widest text-[#fa243c] uppercase">ACTIVE ALBUM</span>
              <h1 className="text-lg font-bold tracking-tight text-white/95 leading-tight mt-0.5">{track.album}</h1>
              <p className="text-[11px] text-white/45 mt-0.5">{track.artist} • Lo-Fi Chill</p>
            </div>
          </div>

          {/* Interactive Track List Table */}
          <div>
            <div className="text-[9px] font-bold text-white/35 uppercase tracking-widest mb-2 border-b border-white/[0.05] pb-1">
              Tracks
            </div>

            <div className="space-y-0.5">
              {tracks.map((t, idx) => {
                const isActive = idx === currentTrackIndex;
                return (
                  <div
                    key={idx}
                    onClick={() => selectTrack(idx)}
                    className={`flex items-center justify-between p-2.5 rounded-lg transition-all cursor-pointer group ${isActive
                        ? 'bg-white/[0.06] border border-white/[0.08]'
                        : 'hover:bg-white/[0.03] border border-transparent'
                      }`}
                  >
                    {/* Index, Cover & Title */}
                    <div className="flex items-center gap-3 min-w-0">
                      <div className="w-4 flex items-center justify-center">
                        {isActive && playback.isPlaying ? (
                          <div className="w-1.5 h-1.5 rounded-full bg-[#fa243c]" />
                        ) : (
                          <span className="text-[10px] text-white/30 group-hover:hidden">{idx + 1}</span>
                        )}
                        <Play size={10} fill="currentColor" stroke="none" className="hidden group-hover:block text-white" />
                      </div>

                      <img src={t.cover} alt="track art" className="w-6 h-6 rounded-[2.5px] object-cover shrink-0" />

                      <div className="flex items-center gap-1.5 min-w-0">
                        <span className={`text-[11px] truncate font-medium ${isActive ? 'text-[#fa243c]' : 'text-white/85'}`}>
                          {t.title}
                        </span>
                        {t.explicit && (
                          <span className="bg-white/10 text-white/40 text-[7px] font-bold px-0.75 py-0.25 rounded shrink-0">E</span>
                        )}
                      </div>
                    </div>

                    {/* Artist & Duration */}
                    <div className="flex items-center gap-4 text-[10px]">
                      <span className="text-white/40 group-hover:text-white/60 transition-colors">{t.artist}</span>
                      <span className="text-white/30 font-mono text-[9px] w-8 text-right">{t.durationStr}</span>
                    </div>

                  </div>
                );
              })}
            </div>
          </div>

        </div>

      </div>

    </div>
  );
}
