import { useState } from "react";
import {
	Check,
	ChevronRight,
	ExternalLink,
	LoaderCircle,
	ShieldAlert,
	Terminal,
	X
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import type { CodexSession, CodexStatus } from "../hooks/useCodexSessions";
import "./CodexIsland.css";

const labels: Record<CodexStatus, string> = {
	idle: "Ready",
	running: "Working",
	awaiting_approval: "Needs Allow",
	completed: "Finished",
	interrupted: "Interrupted",
	unknown: "No recent signal"
};

function StatusIcon({ status }: { status: CodexStatus }) {
	if (status === "awaiting_approval") return <ShieldAlert size={14} />;
	if (status === "running") return <LoaderCircle size={14} className="codex-spinner" />;
	if (status === "completed") return <Check size={14} />;
	return <span className="codex-status-dot" />;
}

export function CodexIsland({
	expanded,
	sessions,
	selected,
	select,
	installed,
	loading,
	error,
	refresh,
	close
}: {
	expanded: boolean;
	sessions: CodexSession[];
	selected: CodexSession | null;
	select: (id: string) => void;
	installed: boolean;
	loading: boolean;
	error: string;
	refresh: () => Promise<void>;
	close: () => void;
}) {
	const [busy, setBusy] = useState(false);
	const [actionError, setActionError] = useState("");
	const running = sessions.filter((s) => s.status === "running").length;
	const approvals = sessions.filter((s) => s.status === "awaiting_approval");
	const pending = selected?.status === "awaiting_approval";
	const open = async () => {
		if (!selected || busy) return;
		setBusy(true);
		setActionError("");
		try {
			await invoke("open_codex_chat", { id: selected.id });
		} catch (e) {
			setActionError(String(e));
		} finally {
			setBusy(false);
		}
	};
	const connect = async () => {
		setBusy(true);
		setActionError("");
		try {
			await invoke("configure_codex_bridge", { install: true });
			await refresh();
		} catch (e) {
			setActionError(String(e));
		} finally {
			setBusy(false);
		}
	};

	if (!expanded)
		return (
			<div className={`codex-compact codex-${selected?.status ?? "idle"}`}>
				<Terminal size={15} />
				<span className="codex-compact-title">{selected?.title ?? "Codex"}</span>
				<StatusIcon status={selected?.status ?? "idle"} />
				<span className="codex-compact-state">
					{selected ? labels[selected.status] : installed ? "Ready" : "Connect"}
				</span>
				{approvals.length > 0 && !pending && (
					<span
						className="codex-pending-count"
						aria-label={`${approvals.length} chats need approval`}
					>
						{approvals.length}
					</span>
				)}
			</div>
		);

	return (
		<section className="codex-island" aria-label="Codex chats">
			<header className="codex-header">
				<Terminal size={16} />
				<strong>Codex</strong>
				<span className="codex-header-count">
					{approvals.length
						? `${approvals.length} need Allow`
						: running
							? `${running} working`
							: "Local chats"}
				</span>
				<button className="codex-icon-button" onClick={close} aria-label="Back to status">
					<X size={14} />
				</button>
			</header>
			{sessions.length > 0 && selected ? (
				<>
					<div className="codex-session-list" aria-label="Choose a chat">
						{sessions.map((session) => (
							<button
								key={session.id}
								className={`codex-session codex-${session.status} ${selected.id === session.id ? "selected" : ""}`}
								onClick={() => {
									select(session.id);
									setActionError("");
								}}
								aria-pressed={selected.id === session.id}
							>
								<StatusIcon status={session.status} />
								<span className="codex-session-title" title={session.title}>
									{session.title}
								</span>
								<span className="codex-session-state">{labels[session.status]}</span>
								<ChevronRight size={12} />
							</button>
						))}
					</div>
					<div className={`codex-detail codex-${selected.status}`}>
						<div className="codex-detail-label">
							<StatusIcon status={selected.status} />
							<span>{labels[selected.status]}</span>
							<time title={new Date(selected.updatedAt).toLocaleString()}>
								{new Date(selected.updatedAt).toLocaleTimeString([], {
									hour: "2-digit",
									minute: "2-digit"
								})}
							</time>
						</div>
						<p title={selected.detail}>{selected.detail || "Open Codex to view this chat."}</p>
						<div className="codex-detail-footer">
							<span title={selected.project}>{selected.project || "Local session"}</span>
							<button
								onClick={() => {
									void open();
								}}
								disabled={busy}
								className={pending ? "codex-approval-button" : "codex-open-button"}
							>
								{busy ? "Opening…" : pending ? "Review in Codex" : "Open chat"}
								<ExternalLink size={12} />
							</button>
						</div>
					</div>
				</>
			) : (
				<div className="codex-empty">
					<Terminal size={28} strokeWidth={1.4} />
					<strong>
						{loading
							? "Reading Codex…"
							: installed
								? "Ready for your next task"
								: "Keep your work in view"}
					</strong>
					<p>
						{installed
							? "Trust Bloom’s hook in Codex, then start or resume a local Codex / Work chat."
							: "Connect Codex to see local chats, task activity and approval requests here."}
					</p>
					{!installed && !loading && (
						<button
							onClick={() => {
								void connect();
							}}
							disabled={busy}
							className="codex-open-button"
						>
							{busy ? "Connecting…" : "Connect Codex"}
						</button>
					)}
					<span>Work Cloud is not connected.</span>
				</div>
			)}
			{(error || actionError) && (
				<div className="codex-error" role="alert">
					{actionError || error}
					<button
						onClick={() => {
							setActionError("");
							void refresh();
						}}
					>
						Retry
					</button>
				</div>
			)}
		</section>
	);
}
