/*
 * Earbud cases, drawn from their geometry.
 *
 * Each case is a footprint (a rounded rectangle, W wide and D deep) cut at a
 * seam: the base below it, the lid above it, hinged on the back edge. The lid
 * is rotated about that hinge and every shape is projected from a camera
 * looking down at PITCH, so the lid swings back exactly as a real one does:
 * its outer face turns away and its inner face, with the pockets that close
 * over the buds, comes into view past upright. The drawing itself stays flat:
 * one gradient per surface, as the rest of the device art.
 *
 * Coordinates: x right, y towards the back, z up; units are the viewBox's.
 */

type Vec3 = [number, number, number];
type Vec2 = [number, number];

interface CaseShape {
	W: number;
	D: number;
	H: number;
	seam: number;
	r: number;
	foot: number;
	top: number;
	buds: "stem" | "round";
	budX: number;
	inner: "light" | "dark";
	dy?: number;
}

export type CaseKind = "stem" | "round";

const CASES: Record<CaseKind, CaseShape> = {
	// After the AirPods Pro: wide, with a lid a third tall.
	stem: { W: 62, D: 22, H: 44, seam: 29, r: 10.6, foot: 13, top: 11, buds: "stem", budX: 16, inner: "light" },
	// After the Galaxy Buds2: a near-square footprint, low, very round, with
	// the inside in a contrasting dark tone.
	round: { W: 54, D: 46, H: 30, seam: 19, r: 19, foot: 10, top: 9, buds: "round", budX: 11.5, inner: "dark", dy: 9 }
};

const PITCH = (26 * Math.PI) / 180;
// Just past upright, where the hinge holds the lid open.
const OPEN = (-108 * Math.PI) / 180;
const NS = "http://www.w3.org/2000/svg";

// Points around the footprint at height z, inset by `inset`.
function ring(c: CaseShape, z: number, inset: number): Vec3[] {
	const pts: Vec3[] = [];
	const hw = c.W / 2 - inset;
	const hd = c.D / 2 - inset;
	const r = Math.max(0.5, c.r - inset);
	const corners: [number, number, number][] = [
		[hw - r, hd - r, 0],
		[-(hw - r), hd - r, 90],
		[-(hw - r), -(hd - r), 180],
		[hw - r, -(hd - r), 270]
	];
	for (const [cx, cy, a0] of corners) {
		for (let i = 0; i <= 30; i++) {
			const a = ((a0 + i * 3) * Math.PI) / 180;
			pts.push([cx + r * Math.cos(a), cy + r * Math.sin(a), z]);
		}
	}
	return pts;
}

// Rings tracing a rounded edge of radius R: the top (dir 1) curving in to
// height z, or the foot (dir -1) curving in down to it.
function rounded(c: CaseShape, z: number, R: number, dir: 1 | -1): Vec3[][] {
	const rings: Vec3[][] = [];
	for (let k = 0; k <= 24; k++) {
		const t = (k / 24) * (Math.PI / 2);
		rings.push(ring(c, z - dir * R * (1 - Math.sin(t)), R * (1 - Math.cos(t))));
	}
	return rings;
}

// Rotate a lid point about the hinge (the back edge at the seam).
function hinge(c: CaseShape, [x, y, z]: Vec3, a: number): Vec3 {
	const hy = c.D / 2 - 1.5;
	const ry = y - hy;
	const rz = z - c.seam;
	return [x, hy + ry * Math.cos(a) - rz * Math.sin(a), c.seam + ry * Math.sin(a) + rz * Math.cos(a)];
}

function project(c: CaseShape, [x, y, z]: Vec3): Vec2 {
	// Centre the case: its middle height sits at the middle of the art.
	const sy = -(z * Math.cos(PITCH) + y * Math.sin(PITCH));
	return [50 + x, 50 + sy + c.H * 0.5 * Math.cos(PITCH) + 6 + (c.dy ?? 0)];
}

function hull(points: Vec2[]): Vec2[] {
	const p = points.slice().sort((a, b) => a[0] - b[0] || a[1] - b[1]);
	const cross = (o: Vec2, a: Vec2, b: Vec2) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
	const lower: Vec2[] = [];
	const upper: Vec2[] = [];
	for (const q of p) {
		while (lower.length >= 2 && cross(lower[lower.length - 2], lower[lower.length - 1], q) <= 0) lower.pop();
		lower.push(q);
	}
	for (const q of p.reverse()) {
		while (upper.length >= 2 && cross(upper[upper.length - 2], upper[upper.length - 1], q) <= 0) upper.pop();
		upper.push(q);
	}
	return lower.slice(0, -1).concat(upper.slice(0, -1));
}

const path = (pts: Vec2[]) => "M" + pts.map((p) => p[0].toFixed(2) + " " + p[1].toFixed(2)).join("L") + "Z";

function el(tag: string, attrs: Record<string, string | number>, parent: Element): SVGElement {
	const e = document.createElementNS(NS, tag);
	for (const k in attrs) e.setAttribute(k, String(attrs[k]));
	parent.appendChild(e);
	return e;
}

// A circle lying on a horizontal surface (or on the rotated lid).
function disc(c: CaseShape, cx: number, cy: number, z: number, r: number, a?: number): string {
	const pts: Vec2[] = [];
	for (let i = 0; i < 72; i++) {
		const t = (i / 72) * Math.PI * 2;
		let p: Vec3 = [cx + r * Math.cos(t), cy + r * 0.78 * Math.sin(t), z];
		if (a !== undefined) p = hinge(c, p, a);
		pts.push(project(c, p));
	}
	return path(pts);
}

// One bud, drawn as the right-hand one and mirrored for the left so the pair
// is exactly symmetrical. Matched to photos of the real thing.
function drawBud(c: CaseShape, g: Element, side: 1 | -1, x: number, settle: number) {
	const [px, py] = project(c, [x, 0, c.seam]);
	const b = el("g", { transform: `translate(${px} ${py - settle}) scale(${side} 1)` }, g);
	if (c.buds === "stem") {
		// Stem buds (AirPods Pro), seated: the head about half above the rim,
		// the silicone tip turned in to meet the other's, a dark oval vent.
		el("ellipse", { cx: -4, cy: 0.5, rx: 12, ry: 1.8, fill: "#000", opacity: 0.1 }, b);
		el("ellipse", { cx: -8.4, cy: -3.4, rx: 3.2, ry: 3.8, fill: "url(#da-g-body)" }, b);
		el("ellipse", { cx: -10.8, cy: -3, rx: 5.6, ry: 5.2, fill: "url(#da-g-silicone)" }, b);
		el("ellipse", { cx: -10.8, cy: -3, rx: 5.6, ry: 5.2, fill: "url(#da-g-side)", opacity: 0.6 }, b);
		el("path", { d: "M-14.6 -5.4 a5 4.4 0 0 1 6.2 -2", fill: "none", stroke: "#fff", "stroke-opacity": 0.8, "stroke-width": 0.8, "stroke-linecap": "round" }, b);
		el("ellipse", { cx: 0, cy: -4, rx: 9, ry: 8.4, fill: "url(#da-g-body)" }, b);
		el("ellipse", { cx: 0, cy: -4, rx: 9, ry: 8.4, fill: "url(#da-g-side)" }, b);
		el("ellipse", { cx: -0.6, cy: -9.8, rx: 4.2, ry: 1.5, fill: "var(--hi)", opacity: 0.85 }, b);
		const vent = el("g", { transform: "rotate(28 3.8 -8.4)" }, b);
		el("ellipse", { cx: 3.8, cy: -8.4, rx: 3.9, ry: 1.7, fill: "#000", opacity: 0.12 }, vent);
		el("ellipse", { cx: 3.8, cy: -8.4, rx: 3.3, ry: 1.05, fill: "url(#da-g-mesh)" }, vent);
		el("path", { d: "M1 -7.6 Q3.8 -6.9 6.6 -7.6", fill: "none", stroke: "#fff", "stroke-opacity": 0.55, "stroke-width": 0.35, "stroke-linecap": "round" }, vent);
	} else {
		// Round buds (Galaxy Buds) lie face up in their wells: a glossy dome,
		// tilted slightly towards the front, the tip hidden below.
		el("ellipse", { cx: 0, cy: 0.4, rx: 10.4, ry: 3, fill: "#000", opacity: 0.25 }, b);
		el("ellipse", { cx: 0, cy: -3.4, rx: 10, ry: 8.4, fill: "url(#da-g-body)" }, b);
		el("ellipse", { cx: 0, cy: -3.4, rx: 10, ry: 8.4, fill: "url(#da-g-side)" }, b);
		el("ellipse", { cx: -2.4, cy: -7.2, rx: 4.2, ry: 2.2, fill: "#fff", opacity: 0.75, transform: "rotate(-14 -2.4 -7.2)" }, b);
		el("ellipse", { cx: 0.4, cy: -4.4, rx: 7.2, ry: 5.6, fill: "none", stroke: "#000", "stroke-opacity": 0.07, "stroke-width": 0.6 }, b);
		el("circle", { cx: 5.6, cy: -1.4, r: 0.6, fill: "var(--dark)", opacity: 0.6 }, b);
	}
}

/** Draws a case into `g` with its lid at angle `a` (0 shut, OPEN open). */
function drawCase(kind: CaseKind, g: Element, a: number, settle: number, uid: string) {
	const c = CASES[kind];
	g.replaceChildren();
	const P = (p: Vec3) => project(c, p);
	const budX = c.budX;
	const inside =
		c.inner === "light"
			? { floor: "var(--body)", lid: "var(--body)", pocket: "var(--shade)", pocketOpacity: 0.4, well: "var(--shade)", wellOpacity: 0.5 }
			: { floor: "#34363c", lid: "#3b3d43", pocket: "#24252a", pocketOpacity: 0.9, well: "#1c1d21", wellOpacity: 0.9 };

	// The lid's outline from its seam ring and the rounded-in top; its inner
	// face once it has turned towards the camera.
	const lidRings = [ring(c, c.seam, 0), ...rounded(c, c.H, c.top, 1)];
	const lidOutline = hull(lidRings.flat().map((p) => P(hinge(c, p, a))));
	const n = [0, Math.sin(a), -Math.cos(a)];
	const innerVisible = -n[1] * Math.cos(PITCH) + n[2] * Math.sin(PITCH) > 0;
	const lidBehind = a < -1.0;

	const drawLid = () => {
		el("path", { d: path(lidOutline), fill: "url(#da-g-body)" }, g);
		el("path", { d: path(lidOutline), fill: "url(#da-g-side)" }, g);
		if (innerVisible) {
			const inner = ring(c, c.seam, 1.4).map((p) => P(hinge(c, p, a)));
			el("path", { d: path(inner), fill: inside.lid }, g);
			const lip = ring(c, c.seam, 2.8).map((p) => P(hinge(c, p, a)));
			el("path", { d: path(lip), fill: "#000", opacity: 0.06 }, g);
			for (const s of [-1, 1]) {
				el("path", { d: disc(c, s * budX, 0, c.seam, c.buds === "stem" ? 12.5 : 11, a), fill: inside.pocket, opacity: inside.pocketOpacity }, g);
			}
		} else {
			const top = ring(c, c.H, c.top + 1).map((p) => P(hinge(c, p, a)));
			el("path", { d: path(top), fill: "var(--hi)", opacity: 0.55 }, g);
		}
	};

	if (lidBehind) drawLid();

	// Base: outline from its rounded-in foot up to the seam; the rim and the
	// floor of the open case, with the wells; the status light.
	const baseOutline = hull([...rounded(c, 0, c.foot, -1), ring(c, c.seam, 0)].flat().map(P));
	el("path", { d: path(baseOutline), fill: "url(#da-g-body)" }, g);
	el("path", { d: path(baseOutline), fill: "url(#da-g-side)" }, g);
	el("path", { d: path(ring(c, c.seam, 0).map(P)), fill: "var(--hi)" }, g);
	el("path", { d: path(ring(c, c.seam, 1.6).map(P)), fill: inside.floor }, g);
	el("path", { d: path(ring(c, c.seam, 1.6).map(P)), fill: "#000", opacity: 0.08 }, g);
	for (const s of [-1, 1]) {
		el("path", { d: disc(c, s * budX, 0, c.seam, c.buds === "stem" ? 8 : 10.5), fill: inside.well, opacity: inside.wellOpacity }, g);
	}
	const [lx, ly] = P([0, -c.D / 2, c.seam * 0.5]);
	el("circle", { cx: lx, cy: ly, r: 0.9, fill: "var(--dark)" }, g);
	el("circle", { cx: lx, cy: ly, r: 4, fill: "url(#da-g-accent)", class: "accent-glow" }, g);

	// Whatever of a bud is below the rim is inside the case.
	const clip = el("clipPath", { id: `da-rim-${uid}`, clipPathUnits: "userSpaceOnUse" }, g);
	el("rect", { x: -50, y: -100, width: 200, height: 100.6 + P([0, 0, c.seam])[1] }, clip);
	const budLayer = el("g", { "clip-path": `url(#da-rim-${uid})` }, g);
	for (const s of [-1, 1] as const) drawBud(c, budLayer, s, s * budX, settle);

	if (!lidBehind) drawLid();
	// The seam line along the front while the lid is down.
	if (a > -0.15) {
		const front = ring(c, c.seam, 0)
			.filter((p) => p[1] < 0)
			.sort((p, q) => p[0] - q[0])
			.map(P);
		el("path", { d: "M" + front.map((p) => p[0].toFixed(2) + " " + p[1].toFixed(2)).join("L"), fill: "none", stroke: "var(--shade)", "stroke-width": 0.6 }, g);
	}
}

// Timeline: the lid starts swinging after the arrival beat and overshoots a
// little before the hinge holds it; the buds settle as it clears them. On
// disconnect the lid closes.
const ease = (t: number) => 1 - Math.pow(1 - t, 3);
function lidAngle(t: number, phase: "play" | "leave"): number {
	if (phase === "leave") return OPEN * (1 - ease(Math.max(0, Math.min(1, (t - 0.14) / 0.28))));
	const p = Math.max(0, Math.min(1, (t - 0.3) / 0.62));
	return OPEN * (ease(p) + Math.sin(p * Math.PI) * 0.07);
}
function budSettle(t: number, phase: "play" | "leave"): number {
	if (phase === "leave") return 0;
	const p = Math.max(0, Math.min(1, (t - 0.6) / 0.5));
	return 1.6 * Math.sin(p * Math.PI) * (1 - p);
}

let uids = 0;

/**
 * Plays a case's connect ("play") or disconnect ("leave") motion in `g`.
 * Returns a function that stops it. Draws the final frame at once when
 * motion is reduced.
 */
export function animateCase(kind: CaseKind, g: Element, phase: "play" | "leave", reduced = false): () => void {
	const uid = String(++uids);
	if (reduced) {
		drawCase(kind, g, lidAngle(9, phase), 0, uid);
		return () => {};
	}
	let frame = 0;
	const start = performance.now();
	const step = (now: number) => {
		const t = (now - start) / 1000;
		drawCase(kind, g, lidAngle(t, phase), budSettle(t, phase), uid);
		if (t < 1.6) frame = requestAnimationFrame(step);
	};
	frame = requestAnimationFrame(step);
	return () => cancelAnimationFrame(frame);
}
