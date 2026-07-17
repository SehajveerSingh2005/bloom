import { useState, useEffect, useCallback, useRef } from 'react';
import { motion, AnimatePresence, useAnimation } from 'framer-motion';
import { invoke } from '@tauri-apps/api/core';
import { Visualizer } from './App';
import { PlayIcon, PauseIcon, SkipBackIcon, SkipForwardIcon, MusicNoteIcon, SpeakerIcon, HeadphonesIcon } from './icons';

interface MediaInfo {
  title: string;
  artist: string;
  is_playing: boolean;
  has_media: boolean;
  artwork?: string[];
  position_ms?: number;
  duration_ms?: number;
  seek_enabled?: boolean;
  position_updated_at?: number;
}

interface CompactMediaPlayerProps {
  mediaInfo: MediaInfo;
  albumArtUrl: string | null;
  albumArtKey: number;
  isPlaying: boolean;
  volume: number;
  volumeExpanded: boolean;
  onVolumeExpandedChange: (expanded: boolean) => void;
  onTogglePlayPause: () => void;
  onVolumeChange: (vol: number) => void;
  prevFront: ReturnType<typeof useAnimation>;
  prevBack: ReturnType<typeof useAnimation>;
  nextFront: ReturnType<typeof useAnimation>;
  nextBack: ReturnType<typeof useAnimation>;
  onAnimatePrev: () => void;
  onAnimateNext: () => void;
}

const MARQUEE_SPEED = 30;
const MARQUEE_MIN_DURATION = 5;

function TitleMarquee({ title }: { title: string }) {
  const textRef = useRef<HTMLSpanElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const [isOverflowing, setIsOverflowing] = useState(false);
  const [scrollDistance, setScrollDistance] = useState(0);
  const [scrollDuration, setScrollDuration] = useState(8);

  useEffect(() => {
    const check = () => {
      if (textRef.current && containerRef.current) {
        const textWidth = textRef.current.scrollWidth;
        const containerWidth = containerRef.current.clientWidth;
        const overflow = textWidth > containerWidth;
        setIsOverflowing(overflow);
        if (overflow) {
          const distance = textWidth - containerWidth;
          setScrollDistance(distance);
          setScrollDuration(Math.max(distance / MARQUEE_SPEED, MARQUEE_MIN_DURATION));
        }
      }
    };
    check();
    const observer = new ResizeObserver(check);
    if (containerRef.current) observer.observe(containerRef.current);
    return () => observer.disconnect();
  }, [title]);

  return (
    <div ref={containerRef} className="cmp-marquee-wrap">
      <span
        ref={textRef}
        className={`cmp-title ${isOverflowing ? 'cmp-marquee' : ''}`}
        style={{
          '--scroll-distance': isOverflowing ? `-${scrollDistance}px` : undefined,
          '--scroll-duration': `${scrollDuration}s`,
        } as React.CSSProperties}
      >
        {title}
      </span>
    </div>
  );
}

function formatTime(ms: number): string {
  if (ms <= 0) return '0:00';
  const totalSeconds = Math.floor(ms / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, '0')}`;
}

function formatRemaining(elapsed: number, duration: number): string {
  const remaining = Math.max(0, duration - elapsed);
  const totalSeconds = Math.floor(remaining / 1000);
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `-${minutes}:${seconds.toString().padStart(2, '0')}`;
}

export function CompactMediaPlayer({
  mediaInfo,
  albumArtUrl,
  albumArtKey,
  isPlaying,
  volume,
  volumeExpanded,
  onVolumeExpandedChange,
  onTogglePlayPause,
  onVolumeChange,
  prevFront,
  prevBack,
  nextFront,
  nextBack,
  onAnimatePrev,
  onAnimateNext,
}: CompactMediaPlayerProps) {
  const seekRef = useRef<HTMLInputElement>(null);

  const positionMs = mediaInfo.position_ms ?? 0;
  const durationMs = mediaInfo.duration_ms ?? 0;
  const seekEnabled = mediaInfo.seek_enabled ?? false;
  const positionUpdatedAt = mediaInfo.position_updated_at ?? 0;

  // Single source of truth: last known backend position + when it was received
  const backendPosRef = useRef(positionMs);
  const backendTimeRef = useRef(positionUpdatedAt || Date.now());
  const isSeekingRef = useRef(false);

  // Update refs when backend sends new position (ignore during seek)
  useEffect(() => {
    if (!isSeekingRef.current) {
      backendPosRef.current = positionMs;
      backendTimeRef.current = positionUpdatedAt || Date.now();
    }
  }, [positionMs, positionUpdatedAt]);

  // displayPosition is the live, extrapolated position
  const [displayPosition, setDisplayPosition] = useState(() => {
    const elapsed = Date.now() - (positionUpdatedAt || Date.now());
    return Math.min(positionMs + (isPlaying ? elapsed : 0), durationMs || Infinity);
  });

  // Tick: extrapolate position every 250ms
  useEffect(() => {
    const tick = () => {
      if (isSeekingRef.current) return; // don't update during seek
      const elapsed = isPlaying ? Date.now() - backendTimeRef.current : 0;
      const pos = Math.min(backendPosRef.current + elapsed, durationMs || Infinity);
      setDisplayPosition(pos);
    };
    tick();
    const interval = setInterval(tick, 250);
    return () => clearInterval(interval);
  }, [isPlaying, durationMs]);

  // Seek handlers
  const handleSeekStart = useCallback(() => {
    isSeekingRef.current = true;
  }, []);

  const handleSeekChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    setDisplayPosition(Number(e.target.value));
  }, []);

  const handleSeekEnd = useCallback(() => {
    const seekPos = displayPosition;
    isSeekingRef.current = false;
    // Update backend refs to the seek position so extrapolation continues from there
    backendPosRef.current = seekPos;
    backendTimeRef.current = Date.now();
    invoke('media_seek', { positionMs: seekPos }).catch(() => {});
  }, [displayPosition]);

  const progressPercent = durationMs > 0 ? (displayPosition / durationMs) * 100 : 0;

  return (
    <div className="cmp">
      {/* Row 1: Album art + Title/Artist + Visualizer */}
      <div className="cmp-row cmp-row-info">
        <div className="cmp-art" onClick={(e) => { e.stopPropagation(); invoke('open_media_source_app').catch(() => {}); }} style={{ cursor: 'pointer' }}>
          <AnimatePresence mode="wait" initial={false}>
            {albumArtUrl ? (
              <motion.img
                key={`art-${albumArtKey}`}
                src={albumArtUrl}
                alt="Art"
                initial={{ rotateY: 90, opacity: 0 }}
                animate={{ rotateY: 0, opacity: 1 }}
                exit={{ rotateY: -90, opacity: 0 }}
                transition={{ duration: 0.3, ease: "easeInOut" }}
              />
            ) : (
              <motion.div
                key="placeholder"
                className="cmp-art-placeholder"
                initial={{ rotateY: 90, opacity: 0 }}
                animate={{ rotateY: 0, opacity: 1 }}
                exit={{ rotateY: -90, opacity: 0 }}
                transition={{ duration: 0.3, ease: "easeInOut" }}
              >
                <MusicNoteIcon size={22} />
              </motion.div>
            )}
          </AnimatePresence>
        </div>
        <div className="cmp-track">
          <TitleMarquee title={mediaInfo.title} />
          <span className="cmp-artist">{mediaInfo.artist}</span>
        </div>
        <div className="cmp-visualizer">
          <Visualizer isPlaying={isPlaying} bars={5} height={16} />
        </div>
      </div>

      {/* Row 2: Progress bar */}
      {durationMs > 0 && (
        <div className="cmp-row cmp-row-progress">
          <span className="cmp-time">{formatTime(displayPosition)}</span>
          <div className="cmp-track-bar">
            <div className="cmp-track-fill" style={{ width: `${progressPercent}%` }} />
            {seekEnabled && (
              <input
                ref={seekRef}
                type="range"
                className="cmp-seek"
                min={0}
                max={durationMs}
                step={100}
                value={displayPosition}
                onMouseDown={handleSeekStart}
                onTouchStart={handleSeekStart}
                onChange={handleSeekChange}
                onMouseUp={handleSeekEnd}
                onTouchEnd={handleSeekEnd}
                onClick={(e) => e.stopPropagation()}
              />
            )}
          </div>
          <span className="cmp-time">{formatRemaining(displayPosition, durationMs)}</span>
        </div>
      )}

      {/* Row 3: Controls */}
      <div className="cmp-row cmp-row-controls">
        <motion.button
          className="cmp-btn cmp-btn-side"
          onClick={(e) => { e.stopPropagation(); onVolumeExpandedChange(!volumeExpanded); }}
          whileTap={{ scale: 0.9 }}
          title="Volume"
        >
          <SpeakerIcon size={20} muted={volume === 0} />
        </motion.button>

        <div className="cmp-main-controls">
          <motion.button className="cmp-btn" onClick={(e) => { e.stopPropagation(); onAnimatePrev(); }} whileTap={{ scale: 0.9 }}>
            <div className="cmp-slide-wrap">
              <motion.div animate={prevBack} className="cmp-slide-layer"><SkipBackIcon size={24} /></motion.div>
              <motion.div animate={prevFront} className="cmp-slide-layer"><SkipBackIcon size={24} /></motion.div>
            </div>
          </motion.button>

          <motion.button className="cmp-btn cmp-play" onClick={(e) => { e.stopPropagation(); onTogglePlayPause(); }} whileTap={{ scale: 0.95 }}>
            <AnimatePresence mode="wait" initial={false}>
              <motion.div
                key={isPlaying ? "pause" : "play"}
                initial={{ opacity: 0, scale: 0.7 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.7 }}
                transition={{ duration: 0.12 }}
                style={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}
              >
                {isPlaying ? <PauseIcon size={26} /> : <PlayIcon size={28} />}
              </motion.div>
            </AnimatePresence>
          </motion.button>

          <motion.button className="cmp-btn" onClick={(e) => { e.stopPropagation(); onAnimateNext(); }} whileTap={{ scale: 0.9 }}>
            <div className="cmp-slide-wrap">
              <motion.div animate={nextBack} className="cmp-slide-layer"><SkipForwardIcon size={24} /></motion.div>
              <motion.div animate={nextFront} className="cmp-slide-layer"><SkipForwardIcon size={24} /></motion.div>
            </div>
          </motion.button>
        </div>

        <motion.button
          className="cmp-btn cmp-btn-side"
          onClick={(e) => {
            e.stopPropagation();
            invoke('open_sound_settings').catch(() => {});
          }}
          whileTap={{ scale: 0.9 }}
          title="Audio Output"
        >
          <HeadphonesIcon size={20} />
        </motion.button>
      </div>

      {/* Volume expansion */}
      <AnimatePresence>
        {volumeExpanded && (
          <motion.div
            className="cmp-expand"
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: 36, opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            transition={{ type: "spring", stiffness: 500, damping: 30 }}
          >
            <div className="cmp-volume-row">
              <SpeakerIcon size={16} style={{ opacity: 0.5, flexShrink: 0 }} />
              <div className="cmp-volume-track">
                <div className="cmp-volume-fill" style={{ width: `${volume * 100}%` }} />
                <input
                  type="range"
                  className="cmp-volume-slider"
                  min={0} max={1} step={0.01} value={volume}
                  onChange={(e) => onVolumeChange(parseFloat(e.target.value))}
                  onClick={(e) => e.stopPropagation()}
                />
              </div>
              <SpeakerIcon size={16} muted={false} style={{ opacity: 0.5, flexShrink: 0 }} />
            </div>
          </motion.div>
        )}
      </AnimatePresence>

    </div>
  );
}
