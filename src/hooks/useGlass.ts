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
 * only when they changed, so the glass follows every resize and animation.
 * Hidden or faded-out elements are left out. Each box is pulled in from its
 * rounded edges so the blur never pokes out of a corner; an edge glued to the
 * top or bottom of the window (square, against the screen edge) stays put.
 */
export function useGlass(getItems: () => GlassItem[], enabled: boolean) {
	const getRef = useRef(getItems);
	getRef.current = getItems;

	useEffect(() => {
		const send = (rects: GlassRect[]) => invoke("set_glass", { rects }).catch(() => {});
		if (!enabled) {
			send([]);
			return;
		}
		let last = "";
		let raf = 0;
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
			if (key !== last) {
				last = key;
				send(rects);
			}
			raf = requestAnimationFrame(tick);
		};
		raf = requestAnimationFrame(tick);
		return () => {
			cancelAnimationFrame(raf);
			send([]);
		};
	}, [enabled]);
}
