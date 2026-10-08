import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { useSettingsSync } from "./useSettingsSync";

interface GlassRect {
	x: number;
	y: number;
	w: number;
	h: number;
}

/** One glass surface: an element, or several that join into one sheet (the
 *  info panel sitting on the dock), so there is no seam between them. */
export type GlassItem = Element | null | undefined | (Element | null | undefined)[];

/** The native blur window has 8 px rounded corners; a box inset by
 *  (R - 8)(1 - 1/√2) from a curve of radius R keeps them hidden inside it. */
const inset = (r: number) => (r > 8 ? Math.ceil((r - 8) * (1 - Math.SQRT1_2)) : 0);

/**
 * The "Glass" setting (Appearance, `bloom-glass`, on by default). Mirrors it
 * on <html class="glass"> so the CSS switches material with it.
 */
export function useGlassEnabled(): boolean {
	const [on, setOn] = useState(() => localStorage.getItem("bloom-glass") !== "false");
	useSettingsSync({ "bloom-glass": (v) => setOn(v !== false && v !== "false") });
	useEffect(() => {
		document.documentElement.classList.toggle("glass", on);
	}, [on]);
	return on;
}

/**
 * Real frosted glass under this window's surfaces (see glass.rs). Every frame
 * the boxes of the surfaces `getItems` returns go to the native blur windows,
 * only when they changed, so the glass follows every animation that grows it.
 * A real collapse (panel rolling into the bar, dock hiding to its pill) is
 * different: DWM paints a resized blur window black for a frame, which left a
 * dark trail behind the closing panel. So the blur steps aside on a large
 * area drop and comes back once the shape has held still for a few frames.
 * Small dips from spring overshoot stay live, or the glass would park through
 * every reveal.
 * Hidden or faded-out elements are left out. Each box is pulled in from its
 * rounded edges so the blur never pokes out of a corner; an edge glued to the
 * top or bottom of the window (square, against the screen edge) stays put.
 */
export function useGlass(getItems: () => GlassItem[], enabled: boolean) {
	const getRef = useRef(getItems);
	getRef.current = getItems;

	useEffect(() => {
		// Rust answers whether Windows really blurs (transparency effects on,
		// energy saver off). If not, the page keeps its plain look: <html
		// class="no-blur"> turns the glass styling off.
		const send = (rects: GlassRect[]) =>
			invoke<boolean>("set_glass", { rects })
				.then((live) => document.documentElement.classList.toggle("no-blur", !live))
				.catch(() => {});
		if (!enabled) {
			send([]);
			return;
		}
		let shown = "";
		let shownArea = 0;
		let settling = false;
		let candidate = "";
		let still = 0;
		let raf = 0;
		// Re-sent now and then even when nothing moved, so plugging in or
		// leaving energy saver brings the glass back.
		let sinceSend = 0;
		const measure = (el: Element | null | undefined) => {
			if (!(el instanceof HTMLElement)) return null;
			const b = el.getBoundingClientRect();
			if (b.width < 2 || b.height < 2) return null;
			const cs = getComputedStyle(el);
			if (cs.visibility === "hidden" || cs.display === "none" || parseFloat(cs.opacity) < 0.05) return null;
			// Radii are in the element's own pixels; a transform scales them too.
			const k = el.offsetWidth > 0 ? b.width / el.offsetWidth : 1;
			const radius = (a: string, c: string) => Math.max(parseFloat(a) || 0, parseFloat(c) || 0) * k;
			return {
				b,
				top: inset(radius(cs.borderTopLeftRadius, cs.borderTopRightRadius)),
				bottom: inset(radius(cs.borderBottomLeftRadius, cs.borderBottomRightRadius))
			};
		};
		const tick = () => {
			const rects: GlassRect[] = [];
			for (const item of getRef.current()) {
				const parts = (Array.isArray(item) ? item : [item]).map(measure).filter((m) => m !== null);
				if (parts.length === 0) continue;
				const left = Math.min(...parts.map((p) => p.b.left));
				const right = Math.max(...parts.map((p) => p.b.right));
				const first = parts.reduce((a, p) => (p.b.top < a.b.top ? p : a));
				const lastPart = parts.reduce((a, p) => (p.b.bottom > a.b.bottom ? p : a));
				const side = Math.max(first.top, lastPart.bottom);
				const y0 = first.b.top <= 1 ? first.b.top : first.b.top + first.top;
				const y1 =
					lastPart.b.bottom >= window.innerHeight - 1 ? lastPart.b.bottom : lastPart.b.bottom - lastPart.bottom;
				rects.push({
					x: Math.round(left + side),
					y: Math.round(y0),
					w: Math.round(right - left - 2 * side),
					h: Math.round(y1 - y0)
				});
			}
			const key = JSON.stringify(rects);
			const area = rects.reduce((a, r) => a + r.w * r.h, 0);
			// A real collapse (the panel rolling back into the bar, the dock
			// hiding to its pill): DWM paints a resized blur window black for
			// a frame, which trailed the closing panel. So step aside at once
			// and come back once the shape has held still. Anything smaller
			// is spring overshoot or corner-inset jitter mid-reveal: parking
			// on those left the glass flickering through every animation, so
			// they stay live and track every frame.
			if (!settling && shownArea > 0 && key !== shown && area < shownArea * 0.6) {
				settling = true;
				send([]);
				candidate = key;
				still = 0;
			}
			if (settling) {
				if (key !== candidate) {
					candidate = key;
					still = 0;
				} else if (++still >= 8) {
					settling = false;
					shown = key;
					shownArea = area;
					send(rects);
				}
			} else if (key !== shown || ++sinceSend > 150) {
				shown = key;
				shownArea = area;
				sinceSend = 0;
				send(rects);
			}
			raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
		return () => {
			cancelAnimationFrame(raf);
			send([]);
			document.documentElement.classList.remove("no-blur");
		};
	}, [enabled]);
}
