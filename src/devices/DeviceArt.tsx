import { useLayoutEffect, useRef } from "react";
import { DEVICE_ART, DEVICE_ART_DEFS, type DeviceArtKey } from "./art";
import { animateCase } from "./caseArt";
import "./devices.css";

/**
 * The gradients and patterns every drawing uses; render once per window.
 * Their colours come from the palette tokens, which are resolved where the
 * gradients are defined, so this element carries the tokens too.
 */
export function DeviceArtDefs() {
	return (
		<svg width="0" height="0" className="device-art" style={{ position: "absolute" }} aria-hidden="true">
			<defs dangerouslySetInnerHTML={{ __html: DEVICE_ART_DEFS }} />
		</svg>
	);
}

// The keyboard's keys, in key units (u): a slim function row above a compact
// layout. Keys light left to right, each row a beat behind the one above.
const KEY_ROWS = [
	[1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2],
	[1.5, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1.5],
	[1.75, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2.25],
	[2.25, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 2.75],
	[1, 1, 1.25, 1.25, 5.25, 1.25, 1, 1, 1, 1]
];

function buildKeys(keys: Element) {
	const u = 82 / 15;
	const gap = 0.7;
	const key = (x: number, y: number, w: number, h: number, d: number) => {
		const k = document.createElementNS("http://www.w3.org/2000/svg", "rect");
		k.setAttribute("x", String(x));
		k.setAttribute("y", String(y));
		k.setAttribute("width", String(w));
		k.setAttribute("height", String(h));
		k.setAttribute("rx", "1");
		k.setAttribute("class", "key");
		k.style.setProperty("--d", `${d}s`);
		keys.appendChild(k);
	};
	for (let c = 0; c < 15; c++) key(9 + c * u + gap / 2, 32.6, u - gap, 3.2, 0.4 + c * 0.03);
	KEY_ROWS.forEach((row, r) => {
		let x = 9;
		for (const w of row) {
			key(x + gap / 2, 37.4 + r * 6.6, w * u - gap, 5.8, 0.45 + (x - 9) * 0.006 + r * 0.04);
			x += w * u;
		}
	});
}

// A short bright dash runs along each pulse path (wired earbuds): CSS can't
// slide a dash whose length comes from a variable, so it runs from script.
function playPulses(svg: SVGSVGElement) {
	for (const p of svg.querySelectorAll<SVGPathElement>(".pulse")) {
		const len = p.getTotalLength();
		p.style.strokeDasharray = `10 ${len + 10}`;
		p.animate(
			[
				{ strokeDashoffset: 10, opacity: 1 },
				{ strokeDashoffset: -len * 0.85, opacity: 1, offset: 0.85 },
				{ strokeDashoffset: -len, opacity: 0 }
			],
			{
				duration: Number(p.dataset.duration || 640),
				delay: Number(p.dataset.delay || 560),
				easing: "cubic-bezier(0.4, 0, 0.6, 1)",
				fill: "both"
			}
		);
	}
}

/**
 * One device drawing. `phase` "play" runs its connect animation, "leave" the
 * disconnect; either replays when `runId` changes. `graphite` switches to
 * the dark palette. `still` draws where the connect animation ends, without
 * playing it, for lists.
 */
export function DeviceArt({
	art,
	phase,
	runId,
	size,
	graphite = false,
	still = false
}: {
	art: DeviceArtKey;
	phase: "play" | "leave";
	runId: number;
	size: number;
	graphite?: boolean;
	still?: boolean;
}) {
	const ref = useRef<SVGSVGElement>(null);

	useLayoutEffect(() => {
		const svg = ref.current;
		if (!svg) return;
		svg.innerHTML = DEVICE_ART[art];
		const keys = svg.querySelector("#da-keyboard-keys");
		if (keys) buildKeys(keys);
		for (const p of svg.querySelectorAll<SVGGeometryElement>(".draw")) {
			p.style.setProperty("--len", String(p.getTotalLength() + 1));
		}
		const reduced = still || window.matchMedia("(prefers-reduced-motion: reduce)").matches;
		const caseKind = art === "stem-buds" ? "stem" : art === "round-buds" ? "round" : null;
		const dev = svg.querySelector(".dev");
		const stop = caseKind && dev ? animateCase(caseKind, dev, phase, reduced) : () => {};
		if (phase === "play" && !reduced) playPulses(svg);
		return stop;
	}, [art, phase, runId, still]);

	return (
		<svg
			// A fresh element per run restarts the CSS animations.
			key={runId}
			ref={ref}
			viewBox="0 0 100 100"
			width={size}
			height={size}
			className={`device-art ${phase}${graphite ? " graphite" : ""}${still ? " still" : ""}`}
			style={{ overflow: "visible", flex: "none" }}
			aria-hidden="true"
		/>
	);
}
