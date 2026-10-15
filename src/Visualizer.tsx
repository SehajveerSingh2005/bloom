import { memo, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { useSettingsSync } from "./hooks/useSettingsSync";
import { hexToHsl } from "./theme";

// The audio visualizer, after Kurve (plasma-audio-visualizer): cava-style
// bands from the backend (cava.rs, "audio-visualization") drawn on a canvas.
// Styles: bars (mirrored round bars), wave (a filled, mirrored curve) and
// blocks (stacked segments). Colours: rainbow (hues across the bands,
// drifting), smooth (a gradient around the accent, breathing) or mono (the
// text colour, the default). Settings: bloom-viz-style, bloom-viz-color.

export type VizStyle = "bars" | "wave" | "blocks";
export type VizColor = "rainbow" | "smooth" | "mono";

const REST = 0.12;

function useVizSettings() {
	const [style, setStyle] = useState(
		() => (localStorage.getItem("bloom-viz-style") as VizStyle) || "bars"
	);
	const [color, setColor] = useState(
		() => (localStorage.getItem("bloom-viz-color") as VizColor) || "mono"
	);
	useSettingsSync({
		"bloom-viz-style": (v) => setStyle((String(v ?? "") as VizStyle) || "bars"),
		"bloom-viz-color": (v) => setColor((String(v ?? "") as VizColor) || "mono")
	});
	return { style, color };
}

/** n values from the bands by linear interpolation. */
function resample(bands: number[], n: number) {
	if (bands.length === n) return bands;
	return Array.from({ length: n }, (_, i) => {
		const x = (i / Math.max(1, n - 1)) * (bands.length - 1);
		const a = Math.floor(x);
		const b = Math.min(bands.length - 1, a + 1);
		return bands[a] + (bands[b] - bands[a]) * (x - a);
	});
}

function paint(
	ctx: CanvasRenderingContext2D,
	x0: number,
	x1: number,
	color: VizColor,
	t: number,
	alpha: number
) {
	const root = getComputedStyle(document.documentElement);
	if (color === "mono") return root.getPropertyValue("--bloom-text").trim() || "#fff";
	const g = ctx.createLinearGradient(x0, 0, x1, 0);
	if (color === "rainbow") {
		const drift = (t / 40) % 360;
		for (let i = 0; i <= 6; i++)
			g.addColorStop(i / 6, `hsla(${(drift + i * 50) % 360}, 90%, 62%, ${alpha})`);
	} else {
		let hue = 210;
		try {
			hue = hexToHsl(root.getPropertyValue("--bloom-accent").trim() || "#007aff").h;
		} catch {
			// keep the default
		}
		const breathe = Math.sin(t / 900) * 18;
		g.addColorStop(0, `hsla(${hue + breathe}, 85%, 62%, ${alpha})`);
		g.addColorStop(0.5, `hsla(${hue + 40 + breathe}, 85%, 66%, ${alpha})`);
		g.addColorStop(1, `hsla(${hue + 80 + breathe}, 85%, 62%, ${alpha})`);
	}
	return g;
}

function draw(
	ctx: CanvasRenderingContext2D,
	w: number,
	h: number,
	values: number[],
	style: VizStyle,
	color: VizColor,
	t: number,
	playing: boolean
) {
	ctx.clearRect(0, 0, w, h);
	ctx.globalAlpha = playing ? 0.95 : 0.5;
	const fill = paint(ctx, 0, w, color, t, 1);
	ctx.fillStyle = fill;
	ctx.strokeStyle = fill;
	const mid = h / 2;

	if (style === "wave") {
		// A smooth curve through the bands, mirrored about the middle: filled
		// soft, outlined bright.
		const n = values.length;
		const x = (i: number) => (i / (n - 1)) * w;
		// Tapered to a point at both ends, so it reads as one ribbon.
		const y = (i: number) =>
			Math.max(0.6, ((values[i] * h) / 2) * Math.sin((Math.PI * i) / (n - 1)) ** 0.6);
		ctx.beginPath();
		ctx.moveTo(0, mid - y(0));
		for (let i = 0; i < n - 1; i++) {
			const cx = (x(i) + x(i + 1)) / 2;
			ctx.bezierCurveTo(cx, mid - y(i), cx, mid - y(i + 1), x(i + 1), mid - y(i + 1));
		}
		ctx.lineTo(w, mid + y(n - 1));
		for (let i = n - 1; i > 0; i--) {
			const cx = (x(i) + x(i - 1)) / 2;
			ctx.bezierCurveTo(cx, mid + y(i), cx, mid + y(i - 1), x(i - 1), mid + y(i - 1));
		}
		ctx.closePath();
		const alpha = ctx.globalAlpha;
		ctx.globalAlpha = alpha * 0.5;
		ctx.fill();
		ctx.globalAlpha = alpha;
		ctx.lineWidth = 1.2;
		ctx.stroke();
		return;
	}

	const n = values.length;
	const gap = style === "blocks" ? 1.5 : 1.6;
	const bw = (w - gap * (n - 1)) / n;
	for (let i = 0; i < n; i++) {
		const x = i * (bw + gap);
		if (style === "bars") {
			const bh = Math.max(bw, values[i] * h);
			ctx.beginPath();
			ctx.roundRect(x, mid - bh / 2, bw, bh, bw / 2);
			ctx.fill();
		} else {
			// Blocks: 2px segments stacked from the bottom.
			const seg = 2;
			const step = seg + 1;
			const count = Math.max(1, Math.round((values[i] * h) / step));
			for (let k = 0; k < count; k++) ctx.fillRect(x, h - seg - k * step, bw, seg);
		}
	}
}

export const Visualizer = memo(function Visualizer({
	isPlaying,
	bars = 5,
	height = 20
}: {
	isPlaying: boolean;
	/** Sets the width, as the old bar visualizer did: 6px per bar. */
	bars?: number;
	height?: number;
}) {
	const width = bars * 6;
	const { style, color } = useVizSettings();
	const canvasRef = useRef<HTMLCanvasElement>(null);
	const target = useRef<number[]>([]);

	useEffect(() => {
		if (!isPlaying) {
			target.current = [];
			return;
		}
		const off = listen<{ frequencies: number[] }>("audio-visualization", (e) => {
			target.current = e.payload.frequencies;
		});
		return () => {
			off.then((f) => f());
		};
	}, [isPlaying]);

	// Eases toward the latest frame every display frame; stops once at rest.
	useEffect(() => {
		const canvas = canvasRef.current;
		const ctx = canvas?.getContext("2d");
		if (!canvas || !ctx) return;
		const dpr = window.devicePixelRatio || 1;
		canvas.width = Math.round(width * dpr);
		canvas.height = Math.round(height * dpr);
		ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
		const n =
			style === "wave"
				? 16
				: style === "bars"
					? Math.max(4, Math.floor((width + 1.6) / 4))
					: Math.max(4, Math.floor((width + 1.5) / 4.5));
		let shown = new Array(n).fill(REST);
		let raf = 0;
		const tick = (t: number) => {
			const goal =
				isPlaying && target.current.length ? resample(target.current, n) : new Array(n).fill(REST);
			let moving = false;
			shown = shown.map((v, i) => {
				const g = Math.max(REST, goal[i]);
				const next = v + (g - v) * (g > v ? 0.55 : 0.3);
				if (Math.abs(next - g) > 0.002) moving = true;
				return next;
			});
			draw(ctx, width, height, shown, style, color, t, isPlaying);
			// Colours keep drifting while playing; at rest it draws once and stops.
			if (isPlaying || moving) raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
		return () => cancelAnimationFrame(raf);
	}, [isPlaying, style, color, width, height]);

	return <canvas ref={canvasRef} className="visualizer-canvas" style={{ width, height }} />;
});
