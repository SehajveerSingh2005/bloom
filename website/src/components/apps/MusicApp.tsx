import { useCallback, useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
	Check,
	ChevronRight,
	Clock,
	Disc,
	Ellipsis,
	ExternalLink,
	House,
	ListMusic,
	Mic,
	Music,
	Plus,
	Repeat,
	Repeat1,
	Search,
	Shuffle,
	Sparkles,
	Star,
	Volume2,
	VolumeX,
	X
} from "lucide-react";
import { PauseIcon, PlayIcon, SkipBackIcon, SkipForwardIcon } from "../../icons";
import { loadYouTubeApi } from "../../lib/youtube";
import type { YouTubePlayer } from "../../lib/youtube";
import albumArt from "../../assets/keychain-laalu.jpg";

interface PlaybackState {
	isPlaying: boolean;
	trackTitle: string;
	trackArtist: string;
	currentTime: number;
	duration: number;
	volume: number;
	trackIndex?: number;
	trackCover?: string;
	tracksCount?: number;
}

interface MusicAppProps {
	playback: PlaybackState;
	setPlaybackState: (state: Partial<PlaybackState>) => void;
	setVisualizerData: (frequencies: number[]) => void;
}

interface AlbumTrack {
	title: string;
	artists: string;
	durationMs: number;
	videoId: string;
}

interface TrackEntry {
	track: AlbumTrack;
	index: number;
}

type View = "home" | "songs" | "albums" | "artists" | "recent" | "new" | "favourites" | "search";

const APPLE_RED = "#fa2d48";
const IDLE_LEVELS = [0.15, 0.15, 0.15, 0.15, 0.15];

const ALBUM = {
	title: "Keychain Laalu",
	artist: "Arpit Bala",
	genre: "Indian Pop",
	year: 2025,
	artwork: albumArt,
	playlistUrl: "https://www.youtube.com/playlist?list=OLAK5uy_kyYXVI95Zk22k5CywnOtE19TS3hP4RTJg",
	tracks: [
		{
			title: "Pyari Amaanat",
			artists: "Arpit Bala, A.O.D. & Angad Virk",
			durationMs: 188571,
			videoId: "pn7-ZM81hQM"
		},
		{
			title: "Chuppi",
			artists: "Arpit Bala, sufr, Adil, A.O.D. & Angad Virk",
			durationMs: 253441,
			videoId: "UJukT1qKH3s"
		},
		{
			title: "Champakali",
			artists: "Arpit Bala, Natiq, toorjo dey & Angad Virk",
			durationMs: 135273,
			videoId: "hI0F69b7BYk"
		},
		{
			title: "Taaron Se",
			artists: "Arpit Bala, A.O.D., Angad Virk & Karan Kanchan",
			durationMs: 267142,
			videoId: "WrczwHORF60"
		},
		{
			title: "Daraaz Mein",
			artists: "Arpit Bala, A.O.D. & Angad Virk",
			durationMs: 219375,
			videoId: "qXNcrFshDNE"
		},
		{
			title: "Kaise Manaye",
			artists: "Arpit Bala, Adil, Karan Kanchan & A.O.D.",
			durationMs: 206893,
			videoId: "GFAE0q_5Aig"
		},
		{
			title: "Best Friend",
			artists: "Arpit Bala, pho, Adil & NEVERSOBER",
			durationMs: 221500,
			videoId: "dfWBmrc3ZQQ"
		},
		{
			title: "Rakhlo Tum Chupaake",
			artists: "Arpit Bala & Adil",
			durationMs: 205800,
			videoId: "slN2QlYr_-c"
		},
		{ title: "RTC Bonus", artists: "Arpit Bala & Adil", durationMs: 109947, videoId: "pW959vzEFM0" }
	] as AlbumTrack[]
};

const TRACK_ENTRIES: TrackEntry[] = ALBUM.tracks.map((track, index) => ({ track, index }));

const ARTIST_NAMES = Array.from(
	new Set(ALBUM.tracks.flatMap((track) => track.artists.split(/,|&/).map((name) => name.trim())))
)
	.filter(Boolean)
	.sort((a, b) => a.localeCompare(b));

function formatDuration(seconds: number) {
	if (!Number.isFinite(seconds) || seconds <= 0) return "--:--";
	const mins = Math.floor(seconds / 60);
	const secs = Math.floor(seconds % 60);
	return `${mins}:${secs.toString().padStart(2, "0")}`;
}

function pickDifferentIndex(count: number, exclude: number) {
	if (count <= 1) return exclude;
	let next = exclude;
	while (next === exclude) next = Math.floor(Math.random() * count);
	return next;
}

function EqBars({ playing, compact = false }: { playing: boolean; compact?: boolean }) {
	return (
		<div
			className={`flex items-end justify-center gap-[2px] ${compact ? "h-3 w-3.5" : "h-3.5 w-4"}`}
			aria-hidden
		>
			{[0, 1, 2].map((i) => (
				<span
					key={i}
					className="am-eq-bar w-[2.5px] rounded-full"
					style={{
						backgroundColor: APPLE_RED,
						animationDelay: `${i * 0.18}s`,
						animationPlayState: playing ? "running" : "paused"
					}}
				/>
			))}
		</div>
	);
}

function NavItem({
	icon,
	label,
	active,
	onClick
}: {
	icon: ReactNode;
	label: string;
	active: boolean;
	onClick: () => void;
}) {
	return (
		<button
			onClick={onClick}
			className={`flex w-full items-center gap-2.5 rounded-md px-2.5 py-[7.5px] text-left text-[13.5px] transition-colors ${
				active
					? "bg-white/[0.1] text-white"
					: "text-white/70 hover:bg-white/[0.05] hover:text-white"
			}`}
		>
			<span className="flex w-[18px] shrink-0 items-center justify-center">{icon}</span>
			<span className="truncate">{label}</span>
		</button>
	);
}

function TrackList({
	entries,
	currentIndex,
	isPlaying,
	knownDurations,
	onSelect,
	showArtwork = false
}: {
	entries: TrackEntry[];
	currentIndex: number;
	isPlaying: boolean;
	knownDurations: Record<number, number>;
	onSelect: (index: number) => void;
	showArtwork?: boolean;
}) {
	const [menuIndex, setMenuIndex] = useState<number | null>(null);

	useEffect(() => {
		if (menuIndex === null) return;
		const close = () => setMenuIndex(null);
		window.addEventListener("click", close);
		return () => window.removeEventListener("click", close);
	}, [menuIndex]);

	if (entries.length === 0) {
		return <p className="px-3 py-4 text-[13px] text-white/35">No matches.</p>;
	}

	return (
		<div>
			{entries.map(({ track, index }) => {
				const active = index === currentIndex;
				const seconds = knownDurations[index] ?? track.durationMs / 1000;
				return (
					<div
						key={track.videoId}
						onClick={() => onSelect(index)}
						className="group grid cursor-pointer grid-cols-[30px_minmax(0,1fr)_auto_30px] items-center gap-3.5 rounded-md px-3.5 py-[9px] transition-colors hover:bg-white/[0.05]"
					>
						<div className="relative flex h-4 items-center justify-center">
							{active ? (
								<EqBars playing={isPlaying} />
							) : (
								<>
									<span className="text-[13px] tabular-nums text-white/35 group-hover:hidden">
										{index + 1}
									</span>
									<PlayIcon size={10} className="hidden text-white group-hover:block" />
								</>
							)}
						</div>

						<div className="flex min-w-0 items-center gap-3">
							{showArtwork && (
								<img src={ALBUM.artwork} alt="" className="h-9 w-9 shrink-0 rounded object-cover" />
							)}
							<div className="min-w-0">
								<div
									className={`truncate text-[13.5px] font-medium ${active ? "" : "text-white/90"}`}
									style={active ? { color: APPLE_RED } : undefined}
								>
									{track.title}
								</div>
								<div className="truncate text-[12px] text-white/40">{track.artists}</div>
							</div>
						</div>

						<span className="text-[12px] tabular-nums text-white/35">
							{formatDuration(seconds)}
						</span>

						<div className="relative flex justify-end">
							<button
								onClick={(e) => {
									e.stopPropagation();
									setMenuIndex(menuIndex === index ? null : index);
								}}
								aria-label={`More options for ${track.title}`}
								className={`flex h-7 w-7 items-center justify-center rounded-md text-white/60 transition hover:bg-white/10 hover:text-white ${
									menuIndex === index ? "opacity-100" : "opacity-0 group-hover:opacity-100"
								}`}
							>
								<Ellipsis size={15} />
							</button>
							{menuIndex === index && (
								<div
									onClick={(e) => e.stopPropagation()}
									className="absolute right-0 top-8 z-30 w-48 overflow-hidden rounded-xl border border-white/10 bg-[#232326]/95 py-1 shadow-2xl backdrop-blur-xl"
								>
									<button
										onClick={() => {
											onSelect(index);
											setMenuIndex(null);
										}}
										className="flex w-full items-center gap-2.5 px-3.5 py-2 text-left text-[12.5px] text-white/85 hover:bg-white/[0.07]"
									>
										<PlayIcon size={10} /> Play
									</button>
									<a
										href={`https://www.youtube.com/watch?v=${track.videoId}`}
										target="_blank"
										rel="noopener noreferrer"
										className="flex w-full items-center gap-2.5 px-3.5 py-2 text-[12.5px] text-white/85 hover:bg-white/[0.07]"
									>
										<ExternalLink size={13} /> Open on YouTube
									</a>
								</div>
							)}
						</div>
					</div>
				);
			})}
		</div>
	);
}

export default function MusicApp({ playback, setPlaybackState, setVisualizerData }: MusicAppProps) {
	const playerHostRef = useRef<HTMLDivElement | null>(null);
	const playerRef = useRef<YouTubePlayer | null>(null);
	const playerReadyRef = useRef(false);
	const loadedIndexRef = useRef(0);
	const errorCountRef = useRef(0);
	const volumeRef = useRef(playback.volume);

	const [playerReady, setPlayerReady] = useState(false);
	const [view, setView] = useState<View>("home");
	const [query, setQuery] = useState("");
	const [shuffle, setShuffle] = useState(false);
	const [repeat, setRepeat] = useState<"off" | "all" | "one">("off");
	const [muted, setMuted] = useState(false);
	const [libraryAdded, setLibraryAdded] = useState(false);
	const [queueOpen, setQueueOpen] = useState(false);
	const [knownDurations, setKnownDurations] = useState<Record<number, number>>({});

	const currentTrackIndex =
		playback.trackIndex !== undefined && playback.trackIndex < ALBUM.tracks.length
			? playback.trackIndex
			: 0;
	const track = ALBUM.tracks[currentTrackIndex];
	const progress =
		playback.duration > 0 ? Math.min(1, playback.currentTime / playback.duration) : 0;

	const currentIndexRef = useRef(currentTrackIndex);
	const playingRef = useRef(playback.isPlaying);
	const shuffleRef = useRef(shuffle);
	const repeatRef = useRef(repeat);
	const advanceRef = useRef<(manual: boolean) => void>(() => {});

	useEffect(() => {
		currentIndexRef.current = currentTrackIndex;
		playingRef.current = playback.isPlaying;
		shuffleRef.current = shuffle;
		repeatRef.current = repeat;
		volumeRef.current = playback.volume;
	});

	const callPlayer = useCallback((action: (player: YouTubePlayer) => void) => {
		const player = playerRef.current;
		if (!player || !playerReadyRef.current) return;
		try {
			action(player);
		} catch {
			/* player not ready for this call yet */
		}
	}, []);

	const advance = useCallback(
		(manual: boolean) => {
			const count = ALBUM.tracks.length;
			const index = currentIndexRef.current;

			if (!manual && repeatRef.current === "one") {
				callPlayer((player) => {
					player.seekTo(0, true);
					player.playVideo();
				});
				setPlaybackState({ currentTime: 0, isPlaying: true });
				return;
			}

			let nextIndex: number;
			if (shuffleRef.current && count > 1) {
				nextIndex = pickDifferentIndex(count, index);
			} else {
				nextIndex = (index + 1) % count;
			}

			setPlaybackState({ trackIndex: nextIndex, isPlaying: true, currentTime: 0, duration: 0 });
		},
		[callPlayer, setPlaybackState]
	);

	useEffect(() => {
		advanceRef.current = advance;
	}, [advance]);

	useEffect(() => {
		let disposed = false;

		loadYouTubeApi().then((YT) => {
			if (disposed || !playerHostRef.current || playerRef.current) return;
			playerRef.current = new YT.Player(playerHostRef.current, {
				videoId: ALBUM.tracks[0].videoId,
				playerVars: {
					controls: 0,
					disablekb: 1,
					modestbranding: 1,
					rel: 0,
					playsinline: 1,
					iv_load_policy: 3,
					fs: 0,
					origin: window.location.origin
				},
				events: {
					onReady: (event) => {
						playerReadyRef.current = true;
						setPlayerReady(true);
						try {
							event.target.setVolume(Math.round(volumeRef.current * 100));
							const startIndex = currentIndexRef.current;
							if (startIndex !== 0) {
								loadedIndexRef.current = startIndex;
								if (playingRef.current) {
									event.target.loadVideoById({ videoId: ALBUM.tracks[startIndex].videoId });
								} else {
									event.target.cueVideoById({ videoId: ALBUM.tracks[startIndex].videoId });
								}
							} else if (playingRef.current) {
								event.target.playVideo();
							}
						} catch {
							/* ignore */
						}
					},
					onStateChange: (event) => {
						if (event.data === YT.PlayerState.PLAYING) {
							errorCountRef.current = 0;
							if (!playingRef.current) setPlaybackState({ isPlaying: true });
						} else if (event.data === YT.PlayerState.PAUSED) {
							if (playingRef.current) setPlaybackState({ isPlaying: false });
						} else if (event.data === YT.PlayerState.ENDED) {
							advanceRef.current(false);
						}
					},
					onError: () => {
						errorCountRef.current += 1;
						if (errorCountRef.current > ALBUM.tracks.length) {
							setPlaybackState({ isPlaying: false });
							return;
						}
						advanceRef.current(true);
					}
				}
			});
		});

		return () => {
			disposed = true;
			playerReadyRef.current = false;
			try {
				playerRef.current?.destroy();
			} catch {
				/* ignore */
			}
			playerRef.current = null;
		};
	}, [setPlaybackState]);

	useEffect(() => {
		if (!playerReady || loadedIndexRef.current === currentTrackIndex) return;
		loadedIndexRef.current = currentTrackIndex;
		const videoId = track.videoId;
		callPlayer((player) => {
			if (playingRef.current) player.loadVideoById({ videoId });
			else player.cueVideoById({ videoId });
		});
	}, [currentTrackIndex, playerReady, track.videoId, callPlayer]);

	useEffect(() => {
		if (!playerReady) return;
		callPlayer((player) => {
			if (playback.isPlaying) player.playVideo();
			else player.pauseVideo();
		});
	}, [playback.isPlaying, playerReady, callPlayer]);

	useEffect(() => {
		callPlayer((player) => player.setVolume(Math.round(playback.volume * 100)));
	}, [playback.volume, playerReady, callPlayer]);

	useEffect(() => {
		callPlayer((player) => {
			if (muted) player.mute();
			else player.unMute();
		});
	}, [muted, playerReady, callPlayer]);

	useEffect(() => {
		if (!playback.isPlaying || !playerReady) return;
		const id = window.setInterval(() => {
			const player = playerRef.current;
			if (!player) return;
			try {
				const time = player.getCurrentTime();
				if (!Number.isFinite(time)) return;
				const duration = player.getDuration();
				if (duration > 0) {
					const rounded = Math.round(duration);
					setKnownDurations((prev) =>
						prev[currentIndexRef.current] === rounded
							? prev
							: { ...prev, [currentIndexRef.current]: rounded }
					);
					setPlaybackState({ currentTime: time, duration });
				} else {
					setPlaybackState({ currentTime: time });
				}
			} catch {
				/* ignore */
			}
		}, 500);
		return () => window.clearInterval(id);
	}, [playback.isPlaying, playerReady, setPlaybackState]);

	useEffect(() => {
		if (!playback.isPlaying) {
			setVisualizerData(IDLE_LEVELS);
			return;
		}
		let bands = [0.35, 0.45, 0.4, 0.45, 0.35];
		const id = window.setInterval(() => {
			bands = bands.map((value, i) => {
				const target = 0.2 + Math.random() * 0.7 * (1 - Math.abs(i - 2) * 0.08);
				return Math.round((value + (target - value) * 0.45) * 100) / 100;
			});
			setVisualizerData([...bands]);
		}, 110);
		return () => window.clearInterval(id);
	}, [playback.isPlaying, setVisualizerData]);

	useEffect(() => {
		setPlaybackState({
			trackTitle: track.title,
			trackArtist: ALBUM.artist,
			trackCover: ALBUM.artwork,
			trackIndex: currentTrackIndex,
			tracksCount: ALBUM.tracks.length
		});
	}, [currentTrackIndex, track, setPlaybackState]);

	useEffect(() => {
		if (!queueOpen) return;
		const close = () => setQueueOpen(false);
		window.addEventListener("click", close);
		return () => window.removeEventListener("click", close);
	}, [queueOpen]);

	const togglePlay = () => {
		const next = !playback.isPlaying;
		callPlayer((player) => {
			if (next) player.playVideo();
			else player.pauseVideo();
		});
		setPlaybackState({ isPlaying: next });
	};

	const selectTrack = (index: number) => {
		if (index === currentIndexRef.current) {
			callPlayer((player) => {
				player.seekTo(0, true);
				player.playVideo();
			});
			setPlaybackState({ currentTime: 0, isPlaying: true });
			return;
		}
		loadedIndexRef.current = index;
		callPlayer((player) => player.loadVideoById({ videoId: ALBUM.tracks[index].videoId }));
		setPlaybackState({ trackIndex: index, isPlaying: true, currentTime: 0, duration: 0 });
	};

	const previous = () => {
		const player = playerRef.current;
		let time = playback.currentTime;
		try {
			time = player ? player.getCurrentTime() : time;
		} catch {
			/* ignore */
		}
		if (time > 3) {
			callPlayer((p) => p.seekTo(0, true));
			setPlaybackState({ currentTime: 0 });
			return;
		}
		const prevIndex = (currentIndexRef.current - 1 + ALBUM.tracks.length) % ALBUM.tracks.length;
		setPlaybackState({ trackIndex: prevIndex, isPlaying: true, currentTime: 0, duration: 0 });
	};

	const seek = (value: number) => {
		callPlayer((player) => player.seekTo(value, true));
		setPlaybackState({ currentTime: value });
	};

	const changeVolume = (value: number) => {
		if (value > 0) setMuted(false);
		setPlaybackState({ volume: value });
	};

	const toggleMute = () => setMuted((prev) => !prev);

	const cycleRepeat = () => {
		setRepeat((prev) => (prev === "off" ? "all" : prev === "all" ? "one" : "off"));
	};

	const shufflePlay = () => {
		setShuffle(true);
		const count = ALBUM.tracks.length;
		const nextIndex = pickDifferentIndex(count, currentIndexRef.current);
		if (nextIndex !== currentIndexRef.current) {
			selectTrack(nextIndex);
		} else {
			setPlaybackState({ isPlaying: true });
		}
	};

	const normalizedQuery = query.trim().toLowerCase();
	const searchResults = normalizedQuery
		? TRACK_ENTRIES.filter(
				({ track: item }) =>
					item.title.toLowerCase().includes(normalizedQuery) ||
					item.artists.toLowerCase().includes(normalizedQuery)
			)
		: [];

	const listProps = {
		currentIndex: currentTrackIndex,
		isPlaying: playback.isPlaying,
		knownDurations,
		onSelect: selectTrack
	};

	const renderContent = () => {
		if (view === "search") {
			return (
				<div className="px-6 pb-28 pt-5">
					<div className="flex items-center gap-2.5 rounded-lg bg-white/[0.07] px-3.5 py-2.5">
						<Search size={15} className="shrink-0 text-white/40" />
						<input
							autoFocus
							value={query}
							onChange={(e) => setQuery(e.target.value)}
							placeholder="Artists, Songs, Albums"
							className="w-full bg-transparent text-[14px] text-white outline-none placeholder:text-white/30"
						/>
						{query && (
							<button onClick={() => setQuery("")} aria-label="Clear search">
								<X size={14} className="text-white/40 hover:text-white" />
							</button>
						)}
					</div>
					<div className="mt-3">
						{normalizedQuery ? (
							<TrackList entries={searchResults} showArtwork {...listProps} />
						) : (
							<p className="px-3 py-4 text-[13px] text-white/35">
								Search for songs, artists, or albums.
							</p>
						)}
					</div>
				</div>
			);
		}

		if (view === "albums" || view === "recent" || view === "new") {
			const heading =
				view === "new" ? "New Releases" : view === "recent" ? "Recently Added" : "Albums";
			return (
				<div className="px-6 pb-28 pt-6">
					<h2 className="mb-4 text-[21px] font-bold tracking-tight">{heading}</h2>
					<button onClick={() => setView("home")} className="group w-[164px] text-left">
						<img
							src={ALBUM.artwork}
							alt={`${ALBUM.title} cover`}
							className="h-[164px] w-[164px] rounded-lg object-cover shadow-[0_18px_44px_-14px_rgba(0,0,0,0.9)] transition group-hover:brightness-110"
						/>
						<div className="mt-2 truncate text-[14px] font-semibold">{ALBUM.title}</div>
						<div className="truncate text-[13px] text-white/45">{ALBUM.artist}</div>
					</button>
				</div>
			);
		}

		if (view === "artists") {
			return (
				<div className="px-6 pb-28 pt-6">
					<h2 className="mb-2 text-[21px] font-bold tracking-tight">Artists</h2>
					<div>
						{ARTIST_NAMES.map((name) => (
							<button
								key={name}
								onClick={() => {
									setQuery(name);
									setView("search");
								}}
								className="flex w-full items-center gap-3 rounded-lg px-2 py-3 text-left transition-colors hover:bg-white/[0.04]"
							>
								<span className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full bg-white/[0.08] text-[14px] font-semibold text-white/70">
									{name.charAt(0)}
								</span>
								<span className="text-[14px] text-white/85">{name}</span>
								<ChevronRight size={15} className="ml-auto text-white/25" />
							</button>
						))}
					</div>
				</div>
			);
		}

		if (view === "songs" || view === "favourites") {
			const heading = view === "favourites" ? "Favourite Songs" : "Songs";
			return (
				<div className="px-6 pb-28 pt-6">
					<h2 className="mb-3 text-[21px] font-bold tracking-tight">{heading}</h2>
					<TrackList entries={TRACK_ENTRIES} showArtwork {...listProps} />
				</div>
			);
		}

		return (
			<div className="pb-28">
				<div className="relative px-6 pt-6">
					<div
						className="pointer-events-none absolute -left-10 -top-24 h-64 w-64 rounded-full opacity-[0.18] blur-[110px]"
						style={{ backgroundColor: APPLE_RED }}
					/>
					<div className="relative">
						<a
							href={ALBUM.playlistUrl}
							target="_blank"
							rel="noopener noreferrer"
							title="Open playlist on YouTube"
							className="absolute right-0 top-0 flex h-9 w-9 items-center justify-center rounded-full border border-white/10 bg-white/[0.04] text-white/60 transition hover:bg-white/[0.09] hover:text-white"
						>
							<Ellipsis size={16} />
						</a>
						<div className="flex items-end gap-6">
							<img
								src={ALBUM.artwork}
								alt={`${ALBUM.title} cover`}
								className="h-[156px] w-[156px] shrink-0 rounded-lg object-cover shadow-[0_22px_54px_-14px_rgba(0,0,0,0.9)]"
							/>
							<div className="min-w-0 pb-0.5">
								<h1 className="truncate text-[30px] font-bold leading-tight tracking-tight">
									{ALBUM.title}
								</h1>
								<button
									onClick={() => {
										setQuery(ALBUM.artist);
										setView("search");
									}}
									className="mt-0.5 block text-[18px] font-medium transition hover:underline"
									style={{ color: APPLE_RED }}
								>
									{ALBUM.artist}
								</button>
								<p className="mt-0.5 text-[11.5px] text-white/40">
									{ALBUM.genre} · {ALBUM.year}
								</p>
								<div className="mt-4 flex items-center gap-3">
									<button
										onClick={shufflePlay}
										title="Shuffle"
										aria-label="Shuffle"
										className="flex h-9 w-9 items-center justify-center rounded-full bg-white/[0.08] text-white/75 transition hover:bg-white/[0.14] hover:text-white active:scale-95"
									>
										<Shuffle size={15} />
									</button>
									<button
										onClick={togglePlay}
										className="flex items-center gap-2 rounded-full bg-white px-6 py-2.5 text-[13.5px] font-semibold text-black transition hover:bg-white/90 active:scale-[0.97]"
									>
										{playback.isPlaying ? <PauseIcon size={12} /> : <PlayIcon size={13} />}
										{playback.isPlaying ? "Pause" : "Play"}
									</button>
									<button
										onClick={() => setLibraryAdded((prev) => !prev)}
										aria-pressed={libraryAdded}
										aria-label={libraryAdded ? "Remove from library" : "Add to library"}
										title={libraryAdded ? "Added to library" : "Add to library"}
										className={`flex h-9 w-9 items-center justify-center rounded-full border transition active:scale-95 ${
											libraryAdded
												? "border-transparent bg-white/90 text-black"
												: "border-white/10 bg-white/[0.04] text-white/70 hover:bg-white/[0.09] hover:text-white"
										}`}
									>
										{libraryAdded ? <Check size={15} /> : <Plus size={15} />}
									</button>
								</div>
							</div>
						</div>
					</div>
				</div>
				<div className="px-6 pt-2">
					<TrackList entries={TRACK_ENTRIES} {...listProps} />
				</div>
			</div>
		);
	};

	const queueRows = ALBUM.tracks.map((item, index) => ({ item, index }));
	const currentSeconds = knownDurations[currentTrackIndex] ?? track.durationMs / 1000;
	const seekMax = Math.max(1, playback.duration || currentSeconds);

	return (
		<div className="relative flex h-full w-full overflow-hidden bg-transparent font-sans text-white select-none">
			<style>{`
        @keyframes amEq {
          0%, 100% { height: 25%; }
          50% { height: 100%; }
        }
        .am-eq-bar { height: 25%; animation: amEq 0.9s ease-in-out infinite; }
        @media (prefers-reduced-motion: reduce) {
          .am-eq-bar { animation: none; height: 60%; }
        }
      `}</style>

			{/* Sidebar */}
			<aside className="relative z-20 flex w-[192px] shrink-0 flex-col border-r border-white/[0.06] bg-black/30 px-2.5 pb-2 pt-4">
				<div className="mb-3 px-2.5">
					<span className="text-[16px] font-semibold tracking-tight text-white">Music</span>
				</div>
				<nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto">
					<NavItem
						icon={<Search size={15} />}
						label="Search"
						active={view === "search"}
						onClick={() => setView("search")}
					/>
					<NavItem
						icon={<House size={15} />}
						label="Home"
						active={view === "home"}
						onClick={() => setView("home")}
					/>
					<NavItem
						icon={<Sparkles size={15} />}
						label="New"
						active={view === "new"}
						onClick={() => setView("new")}
					/>

					<div className="px-2.5 pb-1 pt-4 text-[10.5px] font-semibold uppercase tracking-wider text-white/35">
						Library
					</div>
					<div className="flex items-center gap-1 px-2.5 py-1 text-[12px] font-medium text-white/45">
						<ChevronRight size={13} className="rotate-90" />
						<span>Pins</span>
					</div>
					<NavItem
						icon={<Star size={15} className="fill-current" style={{ color: APPLE_RED }} />}
						label="Favourite Songs"
						active={view === "favourites"}
						onClick={() => setView("favourites")}
					/>
					<NavItem
						icon={
							<img
								src={ALBUM.artwork}
								alt=""
								className="h-[18px] w-[18px] rounded-[3px] object-cover"
							/>
						}
						label={ALBUM.title}
						active={view === "home"}
						onClick={() => setView("home")}
					/>
					<NavItem
						icon={<Clock size={15} />}
						label="Recently Added"
						active={view === "recent"}
						onClick={() => setView("recent")}
					/>
					<NavItem
						icon={<Mic size={15} />}
						label="Artists"
						active={view === "artists"}
						onClick={() => setView("artists")}
					/>
					<NavItem
						icon={<Disc size={15} />}
						label="Albums"
						active={view === "albums"}
						onClick={() => setView("albums")}
					/>
					<NavItem
						icon={<Music size={15} />}
						label="Songs"
						active={view === "songs"}
						onClick={() => setView("songs")}
					/>
					<a
						href={ALBUM.playlistUrl}
						target="_blank"
						rel="noopener noreferrer"
						className="flex items-center gap-2.5 rounded-md px-2.5 py-[7.5px] text-[13.5px] text-white/70 transition-colors hover:bg-white/[0.05] hover:text-white"
					>
						<span className="flex w-[18px] shrink-0 items-center justify-center">
							<ExternalLink size={15} />
						</span>
						<span className="truncate">Open in YouTube</span>
					</a>
				</nav>
				<div className="mt-2 flex items-center gap-2.5 border-t border-white/[0.05] px-2 pt-2.5">
					<div
						className="flex h-7 w-7 items-center justify-center rounded-full text-[10px] font-bold text-black"
						style={{ backgroundColor: APPLE_RED }}
					>
						ZS
					</div>
					<span className="truncate text-[12.5px] font-medium text-white/80">zesty singh</span>
				</div>
			</aside>

			{/* Main */}
			<main className="relative z-10 flex min-w-0 flex-1 flex-col">
				<div className="min-h-0 flex-1 overflow-y-auto">{renderContent()}</div>

				{/* Player pill */}
				<div className="pointer-events-none absolute inset-x-0 bottom-3 z-30 flex justify-center">
					<div className="pointer-events-auto relative w-[480px] max-w-[calc(100%-20px)]">
						<div className="relative flex items-center justify-between rounded-full border border-white/10 bg-[#1b1b1d]/90 px-6 py-3 shadow-[0_20px_50px_-12px_rgba(0,0,0,0.9)] backdrop-blur-2xl">
							<input
								type="range"
								min={0}
								max={seekMax}
								step={0.1}
								value={Math.min(playback.currentTime, seekMax)}
								onChange={(e) => seek(parseFloat(e.target.value))}
								aria-label="Seek"
								className="absolute left-3 right-3 top-[-2px] h-4 w-[calc(100%-24px)] cursor-pointer opacity-0"
							/>
							<div className="pointer-events-none absolute left-3 right-3 top-0 h-[2.5px] overflow-hidden rounded-full bg-white/10">
								<div
									className="h-full rounded-full"
									style={{ width: `${progress * 100}%`, backgroundColor: APPLE_RED }}
								/>
							</div>

							<div className="flex items-center gap-4">
								<button
									onClick={() => setShuffle((prev) => !prev)}
									title="Shuffle"
									aria-label="Shuffle"
									aria-pressed={shuffle}
									className="transition hover:scale-105 active:scale-95"
									style={{ color: shuffle ? APPLE_RED : "rgba(255,255,255,0.65)" }}
								>
									<Shuffle size={16} />
								</button>
								<button
									onClick={previous}
									title="Previous"
									aria-label="Previous track"
									className="text-white/70 transition hover:text-white active:scale-90"
								>
									<SkipBackIcon size={17} />
								</button>
								<button
									onClick={togglePlay}
									title={playback.isPlaying ? "Pause" : "Play"}
									aria-label={playback.isPlaying ? "Pause" : "Play"}
									className="text-white transition hover:scale-105 active:scale-90"
								>
									{playback.isPlaying ? <PauseIcon size={15} /> : <PlayIcon size={16} />}
								</button>
								<button
									onClick={() => advance(true)}
									title="Next"
									aria-label="Next track"
									className="text-white/70 transition hover:text-white active:scale-90"
								>
									<SkipForwardIcon size={17} />
								</button>
								<button
									onClick={cycleRepeat}
									title={
										repeat === "one" ? "Repeat one" : repeat === "all" ? "Repeat all" : "Repeat off"
									}
									aria-label={`Repeat ${repeat}`}
									aria-pressed={repeat !== "off"}
									className="transition hover:scale-105 active:scale-95"
									style={{ color: repeat !== "off" ? APPLE_RED : "rgba(255,255,255,0.65)" }}
								>
									{repeat === "one" ? <Repeat1 size={16} /> : <Repeat size={16} />}
								</button>
							</div>

							<div className="flex items-center gap-3">
								<button
									onClick={(e) => {
										e.stopPropagation();
										setQueueOpen((prev) => !prev);
									}}
									title="Up Next"
									aria-label="Up Next"
									aria-pressed={queueOpen}
									className="transition hover:text-white active:scale-95"
									style={{ color: queueOpen ? APPLE_RED : "rgba(255,255,255,0.65)" }}
								>
									<ListMusic size={16} />
								</button>
								<button
									onClick={toggleMute}
									title={muted ? "Unmute" : "Mute"}
									aria-label={muted ? "Unmute" : "Mute"}
									className="text-white/60 transition hover:text-white"
								>
									{muted ? <VolumeX size={16} /> : <Volume2 size={16} />}
								</button>
								<input
									type="range"
									min={0}
									max={1}
									step={0.01}
									value={playback.volume}
									onChange={(e) => changeVolume(parseFloat(e.target.value))}
									aria-label="Volume"
									className="h-[3px] w-[76px] cursor-pointer appearance-none rounded-full [&::-moz-range-thumb]:h-3 [&::-moz-range-thumb]:w-3 [&::-moz-range-thumb]:border-0 [&::-moz-range-thumb]:bg-transparent [&::-webkit-slider-thumb]:h-3 [&::-webkit-slider-thumb]:w-3 [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-transparent"
									style={{
										background: `linear-gradient(to right, rgba(255,255,255,0.9) 0%, rgba(255,255,255,0.9) ${
											playback.volume * 100
										}%, rgba(255,255,255,0.15) ${playback.volume * 100}%, rgba(255,255,255,0.15) 100%)`
									}}
								/>
							</div>
						</div>

						<AnimatePresence>
							{queueOpen && (
								<motion.div
									initial={{ opacity: 0, y: 10, scale: 0.98 }}
									animate={{ opacity: 1, y: 0, scale: 1 }}
									exit={{ opacity: 0, y: 10, scale: 0.98 }}
									transition={{ duration: 0.16, ease: "easeOut" }}
									onClick={(e) => e.stopPropagation()}
									className="absolute bottom-full right-0 mb-3 max-h-[320px] w-[300px] overflow-y-auto rounded-2xl border border-white/10 bg-[#1b1b1d]/95 p-2 shadow-2xl backdrop-blur-2xl"
								>
									<div className="px-2.5 pb-1 pt-1.5 text-[10.5px] font-semibold uppercase tracking-[0.18em] text-white/40">
										Up Next
									</div>
									{queueRows.map(({ item, index }) => {
										const active = index === currentTrackIndex;
										return (
											<button
												key={item.videoId}
												onClick={() => selectTrack(index)}
												className={`flex w-full items-center gap-3 rounded-xl px-2.5 py-2 text-left transition-colors ${
													active ? "bg-white/[0.08]" : "hover:bg-white/[0.05]"
												}`}
											>
												<img
													src={ALBUM.artwork}
													alt=""
													className="h-9 w-9 shrink-0 rounded-md object-cover"
												/>
												<span className="min-w-0 flex-1">
													<span
														className="block truncate text-[12.5px] font-medium"
														style={
															active ? { color: APPLE_RED } : { color: "rgba(255,255,255,0.85)" }
														}
													>
														{item.title}
													</span>
													<span className="block truncate text-[11px] text-white/40">
														{item.artists}
													</span>
												</span>
												{active ? (
													<EqBars playing={playback.isPlaying} compact />
												) : (
													<span className="shrink-0 text-[11px] tabular-nums text-white/30">
														{formatDuration(knownDurations[index] ?? item.durationMs / 1000)}
													</span>
												)}
											</button>
										);
									})}
								</motion.div>
							)}
						</AnimatePresence>
					</div>
				</div>
			</main>

			{/* Hidden YouTube player host */}
			<div className="pointer-events-none absolute -left-[9999px] top-0 h-[240px] w-[320px] opacity-0">
				<div ref={playerHostRef} />
			</div>
		</div>
	);
}
