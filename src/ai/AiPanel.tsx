import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowUp, Square, X } from "lucide-react";
import type { AiControls } from "./useAi";
import { useAiName } from "./aiName";
import "./ai.css";

const STATUS: Record<string, string> = {
	recording: "Listening",
	transcribing: "Transcribing",
	working: "Working",
	confirm: "Needs your OK",
	done: "Done",
	error: "Couldn't finish"
};

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
}

export function AiPanel({ ai, onClose, focusOnOpen }: Props) {
	const { state, send, stop, answer } = ai;
	const name = useAiName();
	const [text, setText] = useState("");
	const [approveDisabled, setApproveDisabled] = useState(false);
	const rootRef = useRef<HTMLDivElement>(null);
	const inputRef = useRef<HTMLInputElement>(null);
	const approveRef = useRef<HTMLButtonElement>(null);
	const busy = ["recording", "transcribing", "working", "confirm"].includes(state.phase);

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
		<div
			ref={rootRef}
			className="ai-panel"
			data-phase={state.phase}
			onClick={(e) => e.stopPropagation()}
			onKeyDown={(e) => {
				if (e.key !== "Escape") return;
				if (state.confirm) answer(false);
				else onClose();
			}}
		>
			<div className="ai-status">
				<span className="ai-orb-wrap">
					<span className="ai-orb" />
				</span>
				<span className="ai-status-label">{state.phase === "idle" ? name : STATUS[state.phase]}</span>
			</div>
			<div className="ai-body">
				{state.heard && <p className="ai-heard">{state.heard}</p>}
				{state.phase === "working" && state.activity && <p className="ai-activity">{state.activity}</p>}
				{(state.phase === "done" || state.phase === "error") && (
					<p className={`ai-reply ${state.phase}`}>{state.reply}</p>
				)}
			</div>
			{state.confirm && (
				<div className="ai-confirm">
					<div className="ai-confirm-title">{state.confirm.title}</div>
					<pre className="ai-confirm-body">{state.confirm.body}</pre>
					<div className="ai-row">
						<button onClick={() => answer(false)}>Cancel</button>
						<button ref={approveRef} className="primary" disabled={approveDisabled} onClick={() => answer(true)}>
							{state.confirm.kind === "email" ? "Send" : "Run"}
						</button>
					</div>
				</div>
			)}
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
	);
}
