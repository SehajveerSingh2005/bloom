import { useEffect, useState } from "react";
import {
	HeadphonesIcon,
	MusicNoteIcon,
	PauseIcon,
	PlayIcon,
	SkipBackIcon,
	SkipForwardIcon,
	SpeakerIcon,
	VolumeHighIcon,
	VolumeLowIcon
} from "../icons";

const TRACKS = [
	{ title: "Sample track", artist: "Bloom preview" },
	{ title: "Next track", artist: "Bloom preview" },
	{ title: "Another track", artist: "Bloom preview" }
];

function formatTime(seconds: number) {
	return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

export function PreviewMediaPlayer({
	layout,
	onLayoutChange,
	volumeExpanded,
	onVolumeExpandedChange
}: {
	layout: "classic" | "compact";
	onLayoutChange: (layout: "classic" | "compact") => void;
	volumeExpanded: boolean;
	onVolumeExpandedChange: (expanded: boolean) => void;
}) {
	const [track, setTrack] = useState(0);
	const [playing, setPlaying] = useState(true);
	const [position, setPosition] = useState(42);
	const [volume, setVolume] = useState(0.65);
	const [outputOpen, setOutputOpen] = useState(false);
	const duration = 200;

	useEffect(() => {
		if (!playing) return;
		const timer = window.setInterval(() => setPosition((current) => (current >= duration ? 0 : current + 1)), 1000);
		return () => window.clearInterval(timer);
	}, [playing]);

	const skip = (direction: number) => {
		setTrack((current) => (current + direction + TRACKS.length) % TRACKS.length);
		setPosition(0);
	};
	const controls = (
		<>
			<button type="button" aria-label="Previous sample track" onClick={() => skip(-1)}>
				<SkipBackIcon size={layout === "compact" ? 30 : 32} />
			</button>
			<button
				type="button"
				aria-label={playing ? "Pause sample track" : "Play sample track"}
				onClick={() => setPlaying((value) => !value)}
			>
				{playing ? <PauseIcon size={26} /> : <PlayIcon size={26} />}
			</button>
			<button type="button" aria-label="Next sample track" onClick={() => skip(1)}>
				<SkipForwardIcon size={layout === "compact" ? 30 : 32} />
			</button>
		</>
	);
	const volumeSlider = (
		<div className={layout === "compact" ? "cmp-volume-row" : "volume-slider-container"}>
			<VolumeLowIcon size={12} style={{ opacity: 0.5 }} />
			<div className={layout === "compact" ? "cmp-volume-track" : "slider-track-premium"}>
				<div
					className={layout === "compact" ? "cmp-volume-fill" : "slider-progress-fill"}
					style={{ width: `${volume * 100}%` }}
				/>
				<input
					type="range"
					min={0}
					max={1}
					step={0.01}
					value={volume}
					aria-label="Sample volume"
					className={layout === "compact" ? "cmp-volume-slider" : "premium-slider"}
					onChange={(event) => setVolume(Number(event.target.value))}
				/>
			</div>
			<VolumeHighIcon size={14} style={{ opacity: 0.5 }} />
		</div>
	);
	const art = (
		<div className={layout === "compact" ? "cmp-art" : "premium-album-art"}>
			<div className={layout === "compact" ? "cmp-art-placeholder" : "art-placeholder-mini"}>
				<MusicNoteIcon size={layout === "compact" ? 22 : 32} />
			</div>
		</div>
	);

	return (
		<div className="expanded-music-container settings-preview-player" onClick={(event) => event.stopPropagation()}>
			{layout === "compact" ? (
				<div className="cmp">
					<div className="cmp-row cmp-row-info">
						<button
							type="button"
							className="settings-preview-art-button"
							aria-label="Show classic media layout"
							title="Show classic layout"
							onClick={() => onLayoutChange("classic")}
						>
							{art}
						</button>
						<div className="cmp-track">
							<div className="cmp-marquee-wrap">
								<span className="cmp-title">{TRACKS[track].title}</span>
							</div>
							<span className="cmp-artist">{TRACKS[track].artist}</span>
						</div>
						<div
							className={`settings-preview-visualizer${playing ? " playing" : ""}`}
							aria-label={playing ? "Playing" : "Paused"}
						>
							<i />
							<i />
							<i />
							<i />
							<i />
						</div>
					</div>
					<div className="cmp-row cmp-row-progress">
						<span className="cmp-time">{formatTime(position)}</span>
						<div className="cmp-track-bar">
							<div className="cmp-track-fill" style={{ width: `${(position / duration) * 100}%` }} />
							<input
								className="cmp-seek"
								type="range"
								min={0}
								max={duration}
								value={position}
								aria-label="Sample track position"
								onChange={(event) => setPosition(Number(event.target.value))}
							/>
						</div>
						<span className="cmp-time">-{formatTime(duration - position)}</span>
					</div>
					<div className="cmp-row cmp-row-controls">
						<button
							type="button"
							className="cmp-btn cmp-btn-side"
							aria-label="Show sample volume"
							onClick={() => onVolumeExpandedChange(!volumeExpanded)}
						>
							<SpeakerIcon size={22} muted={volume === 0} />
						</button>
						<div className="cmp-main-controls">
							<span className="settings-preview-compact-controls">{controls}</span>
						</div>
						<button
							type="button"
							className="cmp-btn cmp-btn-side"
							aria-label="Sample audio output"
							title="Sample audio output"
							onClick={() => setOutputOpen((value) => !value)}
						>
							<HeadphonesIcon size={22} />
						</button>
					</div>
					{volumeExpanded && <div className="cmp-expand">{volumeSlider}</div>}
					{outputOpen && <div className="settings-preview-output">Speakers · Sample output</div>}
				</div>
			) : (
				<div className="compact-premium-layout">
					<div className="album-art-section">
						<button
							type="button"
							className="settings-preview-art-button"
							aria-label="Show compact media layout"
							title="Show compact layout"
							onClick={() => onLayoutChange("compact")}
						>
							{art}
						</button>
					</div>
					<div className="metadata-controls-section-middle">
						<div className="track-header-row">
							<div className="track-info-middle">
								<div className="premium-title-wrap">
									<span className="premium-title">{TRACKS[track].title}</span>
								</div>
								<span className="premium-artist">{TRACKS[track].artist}</span>
							</div>
							<div className={`settings-preview-visualizer${playing ? " playing" : ""}`}>
								<i />
								<i />
								<i />
								<i />
								<i />
							</div>
						</div>
						<div className="controls-row-sleek settings-preview-classic-controls">{controls}</div>
						{volumeSlider}
					</div>
				</div>
			)}
		</div>
	);
}
