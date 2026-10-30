/** Include follow-up writes queued by continuations of a completed save. */
export async function waitForPendingSaves(getPending: () => Promise<void>): Promise<void> {
	let pending: Promise<void>;
	do {
		pending = getPending();
		await pending;
	} while (pending !== getPending());
}
