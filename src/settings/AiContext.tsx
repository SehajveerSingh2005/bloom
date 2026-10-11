import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Database, FileText, Mail, MessageCircle, Users } from "lucide-react";
import { SettingRow } from "./SettingRow";
import { useAiFlag, useAiSetting } from "./useAiSetting";
import type { SettingRowProps } from "./types";

/** The agent's `context_status`: counts only, never content. */
interface ContextStatus {
	on: boolean;
	counts: Record<string, number>;
	pending: number;
	message: string | null;
	error: string | null;
}

const LABELS: Record<string, string> = { contacts: "contacts", email: "emails", whatsapp: "WhatsApp messages", notes: "note paragraphs" };

const run = (action: string) => invoke("ai_context", { action }).catch(() => {});

export function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: () => void; disabled?: boolean }) {
	return (
		<label className="toggle-switch">
			<input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
			<span className="slider"></span>
		</label>
	);
}

/** A native "are you sure?" before something that can't be undone. */
export async function confirm(title: string, message: string, okLabel: string) {
	const { ask } = await import("@tauri-apps/plugin-dialog");
	return ask(message, { title, kind: "warning", okLabel, cancelLabel: "Cancel" });
}

// The agent drops what is no longer allowed once the setting has landed.
const change = (set: (on: boolean) => Promise<void>, on: boolean) => set(on).then(() => run("apply"));

/** One source the index may read (`bloom-ai-context-<source>`). */
function SourceRow({ source, ...row }: { source: string } & SettingRowProps) {
	const [on, setOn] = useAiFlag(`bloom-ai-context-${source}`);
	return (
		<SettingRow {...row}>
			<Toggle checked={on} onChange={() => change(setOn, !on)} />
		</SettingRow>
	);
}

/** Settings > AI > Context: opt-in indexing, per source, with Delete index. */
export function AiContext({ name }: { name: string }) {
	const [on, setOn] = useAiFlag("bloom-ai-context");
	const [notes, setNotes] = useAiFlag("bloom-ai-context-notes");
	const [notesDir, setNotesDir] = useAiSetting("bloom-ai-context-notes-dir", "");
	const [status, setStatus] = useState<ContextStatus | null>(null);
	const [busy, setBusy] = useState(false);

	useEffect(() => {
		const off = listen<any>("ai-event", ({ payload }) => {
			if (payload.type === "context_status") {
				setStatus(payload);
				setBusy(false);
			}
		});
		run("status");
		return () => {
			off.then((f) => f());
		};
	}, []);

	const total = Object.values(status?.counts ?? {}).reduce((a, b) => a + b, 0);
	const turnOff = async () => {
		const message = `Turning context indexing off deletes the index (${total} items).`;
		if (total > 0 && !(await confirm("Context indexing", message, "Turn off and delete"))) return;
		change(setOn, false);
	};

	const pickNotes = async () => {
		const { open } = await import("@tauri-apps/plugin-dialog");
		const dir = await open({ directory: true, title: "Notes folder" });
		if (typeof dir === "string") setNotesDir(dir).then(() => run("apply"));
	};

	const counts = Object.entries(status?.counts ?? {})
		.map(([source, n]) => `${n} ${LABELS[source] ?? source}`)
		.join(", ");
	const note = status?.error ?? status?.message;

	return (
		<>
			<div className="setting-group-label">Context</div>
			<div className="setting-group">
				<SettingRow
					icon={Database}
					label="Context indexing"
					desc={`Lets ${name} answer questions about your own plans and people ("who am I going with on Friday?") from the sources you tick below. New items are sent to your model provider in hourly batches to find the people, places and times in them. Off deletes the index.`}
				>
					<Toggle checked={on} onChange={() => (on ? turnOff() : change(setOn, true))} />
				</SettingRow>
				<SourceRow source="contacts" icon={Users} label="Contacts" desc="Names, tags and companies of your saved contacts" />
				<SourceRow
					source="email"
					icon={Mail}
					label="Email"
					desc="Subject, sender, recipients, date and the first 500 characters of each email from the last 30 days, read over IMAP at most once a day"
				/>
				<SourceRow
					source="whatsapp"
					icon={MessageCircle}
					label="WhatsApp"
					desc={`Messages that arrive while Bloom is linked are saved on this PC until indexed. This overrides "kept in memory only" for indexed chats. Your own chat with ${name} is never indexed.`}
				/>
				<SettingRow
					icon={FileText}
					label="Notes"
					desc={notesDir ? `.txt and .md files in ${notesDir}` : "A folder of .txt and .md files (1 MB each at most)"}
					divider={on}
				>
					<div className="ai-secret">
						<button className="ai-btn" onClick={pickNotes}>
							{notesDir ? "Change folder" : "Choose folder"}
						</button>
						<Toggle checked={notes} onChange={() => change(setNotes, !notes)} />
					</div>
				</SettingRow>
				{on && (
					<SettingRow
						icon={Database}
						label={`Indexed: ${total} ${total === 1 ? "item" : "items"}`}
						desc={`${counts ? counts + ". " : ""}${status?.pending ? `${status.pending} WhatsApp messages waiting. ` : ""}Only the people, places and times in each item and its first 200 characters are kept.`}
						divider={false}
					>
						<div className="ai-secret">
							<button
								className="ai-btn"
								disabled={busy}
								onClick={() => {
									setBusy(true);
									invoke("ai_context", { action: "sync" }).catch(() => setBusy(false));
								}}
							>
								{busy ? "Indexing..." : "Index now"}
							</button>
							<button
								className="ai-btn"
								onClick={async () => {
									if (await confirm("Context indexing", `Delete the index (${total} items)?`, "Delete index")) run("delete");
								}}
							>
								Delete index
							</button>
						</div>
					</SettingRow>
				)}
			</div>
			{note && <p className="ai-warning">{note}</p>}
		</>
	);
}
