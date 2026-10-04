import { lazy, Suspense, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { AnimatePresence, motion, MotionConfig } from "framer-motion";
import { ArrowUp, Square, X } from "lucide-react";
import type { AiControls } from "./useAi";
import { useAiName } from "./aiName";
import { AiOrb } from "./AiOrb";
import "./ai.css";

const STATUS: Record<string, string> = {
	recording: "Listening",
	transcribing: "Transcribing",
	working: "Thinking",
	confirm: "Needs your OK",
	done: "Done",
	error: "Couldn't finish"
};

// Markdown and KaTeX load with the first open panel, not with the taskbar.
const loadMarkdown = () => import("./Markdown");
const Markdown = lazy(loadMarkdown);

// Lines rise in and fade out; an exiting one leaves the layout at once
// (popLayout), so the notch or dock springs to the new size while it fades.
const LINE = {
	initial: { opacity: 0, y: 6 },
	animate: { opacity: 1, y: 0 },
	exit: { opacity: 0, y: -4, transition: { duration: 0.15 } },
	transition: { type: "spring", stiffness: 450, damping: 32 }
} as const;

/** The notch and dock are no-activate windows: take the keyboard, then focus. */
function takeKeyboard(el: HTMLElement | null) {
	if (!el) return;
	invoke("take_keyboard")
		.catch(() => {})
		.finally(() => el.focus());
}

interface Props {
	ai: AiControls;
	onClose: () => void;
	/** Opened from the dock button: focus the text box. */
	focusOnOpen: boolean;
	/** The panel's natural height in CSS px, whatever box it sits in: the
	 *  notch and dock size themselves to it (clamped) so nothing is cut off. */
	onHeight?: (height: number) => void;
}

export function AiPanel({ ai, onClose, focusOnOpen, onHeight }: Props) {
	const { state, send, stop, answer } = ai;
	const name = useAiName();
	const [text, setText] = useState("");
	const [approveDisabled, setApproveDisabled] = useState(false);
	const rootRef = useRef<HTMLDivElement>(null);
	const inputRef = useRef<HTMLInputElement>(null);
	const approveRef = useRef<HTMLButtonElement>(null);
	const bodyRef = useRef<HTMLDivElement>(null);
	const linesRef = useRef<HTMLDivElement>(null);
	const busy = ["recording", "transcribing", "working", "confirm"].includes(state.phase);
	const thinking = state.phase === "transcribing" || state.phase === "working";

	useEffect(() => {
		loadMarkdown().catch(() => {});
	}, []);

	// Natural height: every row at its own size, the body at its full content
	// (it is the only row that stretches or scrolls). Independent of the box,
	// so the parent can animate to it without a feedback loop.
	const onHeightRef = useRef(onHeight);
	onHeightRef.current = onHeight;
	const lastHeight = useRef(0);
	const measure = useCallback(() => {
		const panel = rootRef.current;
		const lines = linesRef.current;
		if (!onHeightRef.current || !panel || !lines) return;
		const cs = getComputedStyle(panel);
		let h = parseFloat(cs.paddingTop) + parseFloat(cs.paddingBottom);
		let rows = 0;
		for (const el of panel.children) {
			// A card fading out (popLayout) no longer takes space.
			if (!(el instanceof HTMLElement) || getComputedStyle(el).position === "absolute") continue;
			h += el === bodyRef.current ? lines.offsetHeight : el.offsetHeight;
			rows++;
		}
		h = Math.ceil(h + Math.max(0, rows - 1) * (parseFloat(cs.rowGap) || 0));
		if (h !== lastHeight.current) {
			lastHeight.current = h;
			onHeightRef.current(h);
		}
	}, []);
	// Rows come and go with renders; text reflows and Markdown loads without one.
	useLayoutEffect(measure);
	useEffect(() => {
		const observer = new ResizeObserver(measure);
		if (rootRef.current) observer.observe(rootRef.current);
		if (linesRef.current) observer.observe(linesRef.current);
		return () => observer.disconnect();
	}, [measure]);

	useEffect(() => {
		if (focusOnOpen) takeKeyboard(inputRef.current);
	}, [focusOnOpen]);

	// Enter approves (the focused button), Escape declines (handler below).
	// Focus approve only once enabled; while disabled, keep focus on Cancel so Escape works.
	useEffect(() => {
		if (state.phase !== "confirm") return;
		if (!approveDisabled) {
			takeKeyboard(approveRef.current);
		} else {
			// Focus Cancel button (first button in .ai-row) to keep focus on the panel.
			const cancelBtn = rootRef.current?.querySelector(".ai-row button:first-child");
			takeKeyboard(cancelBtn as HTMLElement);
		}
	}, [state.phase, approveDisabled]);

	// Disable approve button for 600ms after confirm card appears to prevent accidental approval.
	useEffect(() => {
		if (!state.confirm) {
			setApproveDisabled(false);
			return;
		}
		setApproveDisabled(true);
		const timer = setTimeout(() => setApproveDisabled(false), 600);
		return () => clearTimeout(timer);
	}, [state.confirm?.id]);

	const submit = (e: React.FormEvent) => {
		e.preventDefault();
		const t = text.trim();
		if (!t || busy) return;
		send(t);
		setText("");
	};

	return (
		<MotionConfig reducedMotion="user">
			<div
				ref={rootRef}
				className="ai-panel"
				data-phase={state.phase}
				data-thinking={thinking || undefined}
				onClick={(e) => e.stopPropagation()}
				onKeyDown={(e) => {
					if (e.key !== "Escape") return;
					if (state.confirm) answer(false);
					else onClose();
				}}
			>
				<div className="ai-status">
					<AiOrb phase={state.phase} size={16} />
					<span className="ai-status-label">
						{state.phase === "idle" ? name : STATUS[state.phase]}
					</span>
					{thinking && (
						<span className="ai-dots" aria-hidden>
							<i />
							<i />
							<i />
						</span>
					)}
					<span className="ai-sweep" aria-hidden />
				</div>
				<div ref={bodyRef} className="ai-body">
					<div ref={linesRef} className="ai-lines">
						<AnimatePresence mode="popLayout" initial={false}>
							{state.heard && (
								<motion.p key={`heard:${state.heard}`} className="ai-heard" {...LINE}>
									{state.heard}
								</motion.p>
							)}
							{state.phase === "working" && state.activity && (
								<motion.p key={`activity:${state.activity}`} className="ai-activity" {...LINE}>
									{state.activity}
								</motion.p>
							)}
							{state.phase === "done" && (
								<motion.div key={`reply:${state.reply}`} className="ai-reply done" {...LINE}>
									<Suspense fallback={<p>{state.reply}</p>}>
										<Markdown text={state.reply} />
									</Suspense>
								</motion.div>
							)}
							{state.phase === "error" && (
								<motion.p key={`error:${state.reply}`} className="ai-reply error" {...LINE}>
									{state.reply}
								</motion.p>
							)}
						</AnimatePresence>
					</div>
				</div>
				<AnimatePresence mode="popLayout" initial={false}>
					{state.confirm && (
						<motion.div key={state.confirm.id} className="ai-confirm" {...LINE}>
							<div className="ai-confirm-title">{state.confirm.title}</div>
							<pre className="ai-confirm-body">{state.confirm.body}</pre>
							<div className="ai-row">
								<button onClick={() => answer(false)}>Cancel</button>
								<button
									ref={approveRef}
									className="primary"
									disabled={approveDisabled}
									onClick={() => answer(true)}
								>
									{state.confirm.kind === "email" ? "Send" : "Run"}
								</button>
							</div>
						</motion.div>
					)}
				</AnimatePresence>
				<form className="ai-input" onSubmit={submit}>
					<input
						ref={inputRef}
						value={text}
						onChange={(e) => setText(e.target.value)}
						onMouseDown={() => takeKeyboard(inputRef.current)}
						placeholder={`Ask ${name}`}
					/>
					{busy ? (
						<button type="button" title="Stop" onClick={stop}>
							<Square size={13} />
						</button>
					) : (
						<button type="submit" title="Send">
							<ArrowUp size={14} />
						</button>
					)}
					<button type="button" title="Close" onClick={onClose}>
						<X size={14} />
					</button>
				</form>
			</div>
		</MotionConfig>
	);
}
