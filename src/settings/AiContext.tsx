import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CalendarDays, Database, FileText, Mail, MessageCircle, Users } from "lucide-react";
import { SettingRow } from "./SettingRow";
import { useAiSetting } from "./AiTab";

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

function Toggle({ checked, onChange, disabled }: { checked: boolean; onChange: () => void; disabled?: boolean }) {
	return (
		<label className="toggle-switch">
			<input type="checkbox" checked={checked} disabled={disabled} onChange={onChange} />
			<span className="slider"></span>
		</label>
	);
}

/** Settings > AI > Context: opt-in indexing, per source, with Delete index. */
export function AiContext({ name }: { name: string }) {
	const [on, setOn] = useAiSetting("bloom-ai-context", "false");
	const [contacts, setContacts] = useAiSetting("bloom-ai-context-contacts", "true");
	const [email, setEmail] = useAiSetting("bloom-ai-context-email", "false");
	const [whatsapp, setWhatsapp] = useAiSetting("bloom-ai-context-whatsapp", "false");
	const [notes, setNotes] = useAiSetting("bloom-ai-context-notes", "false");
	const [notesDir, setNotesDir] = useAiSetting("bloom-ai-context-notes-dir", "");
	const [status, setStatus] = useState<ContextStatus | null>(null);
	const [busy, setBusy] = useState(false);
	const [deleting, setDeleting] = useState(false);

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

	// The agent drops what is no longer allowed once the setting has landed.
	const change = (set: (v: string) => void, value: boolean) => {
		set(value ? "true" : "false");
		setTimeout(() => run("apply"), 300);
	};

	const total = Object.values(status?.counts ?? {}).reduce((a, b) => a + b, 0);
	const turnOff = async () => {
		if (total > 0) {
			const { ask } = await import("@tauri-apps/plugin-dialog");
			const ok = await ask(`Turning context indexing off deletes the index (${total} items).`, {
				title: "Context indexing",
				kind: "warning",
				okLabel: "Turn off and delete",
				cancelLabel: "Cancel"
			});
			if (!ok) return;
		}
		change(setOn, false);
	};

	const pickNotes = async () => {
		const { open } = await import("@tauri-apps/plugin-dialog");
		const dir = await open({ directory: true, title: "Notes folder" });
		if (typeof dir === "string") {
			setNotesDir(dir);
			setTimeout(() => run("apply"), 300);
		}
	};

	const isOn = on === "true";
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
					desc={`Lets ${name} answer questions about your own plans and people ("who am I going with on Friday?") from the sources you tick. Off deletes the index.`}
					divider={isOn}
				>
					<Toggle checked={isOn} onChange={() => (isOn ? turnOff() : change(setOn, true))} />
				</SettingRow>
				{isOn && (
					<>
						<SettingRow icon={Users} label="Contacts" desc="Names, tags and companies of your saved contacts">
							<Toggle checked={contacts === "true"} onChange={() => change(setContacts, contacts !== "true")} />
						</SettingRow>
						<SettingRow
							icon={Mail}
							label="Email"
							desc="Subject, sender, recipients, date and the first 500 characters of each email from the last 30 days, read over IMAP at most once a day"
						>
							<Toggle checked={email === "true"} onChange={() => change(setEmail, email !== "true")} />
						</SettingRow>
						<SettingRow
							icon={MessageCircle}
							label="WhatsApp"
							desc={`Messages that arrive while Bloom is linked are saved on this PC until indexed. This overrides "kept in memory only" for indexed chats. Your own chat with ${name} is never indexed.`}
						>
							<Toggle checked={whatsapp === "true"} onChange={() => change(setWhatsapp, whatsapp !== "true")} />
						</SettingRow>
						<SettingRow icon={CalendarDays} label="Calendar" desc="Needs Outlook sign-in">
							<Toggle checked={false} disabled onChange={() => {}} />
						</SettingRow>
						<SettingRow
							icon={FileText}
							label="Notes"
							desc={notesDir ? `.txt and .md files in ${notesDir}` : "A folder of .txt and .md files (1 MB each at most)"}
						>
							<div className="ai-secret">
								<button className="ai-btn" onClick={pickNotes}>
									{notesDir ? "Change folder" : "Choose folder"}
								</button>
								<Toggle checked={notes === "true"} onChange={() => change(setNotes, notes !== "true")} />
							</div>
						</SettingRow>
						<SettingRow
							icon={Database}
							label={`Indexed: ${total} ${total === 1 ? "item" : "items"}`}
							desc={`${counts ? counts + ". " : ""}${status?.pending ? `${status.pending} WhatsApp messages waiting. ` : ""}Only the people, places and times in each item and a snippet of up to 200 characters are kept.`}
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
									onBlur={() => setDeleting(false)}
									onClick={() => {
										if (!deleting) return setDeleting(true);
										setDeleting(false);
										run("delete");
									}}
								>
									{deleting ? "Delete? Yes" : "Delete index"}
								</button>
							</div>
						</SettingRow>
					</>
				)}
			</div>
			{note && <p className="ai-warning">{note}</p>}
		</>
	);
}
