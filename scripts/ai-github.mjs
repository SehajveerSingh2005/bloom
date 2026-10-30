export function githubClient(env = process.env, request = fetch) {
	if (!env.GITHUB_TOKEN || !/^[\w.-]+\/[\w.-]+$/.test(env.GITHUB_REPOSITORY ?? ""))
		throw new Error("GITHUB_TOKEN and a valid GITHUB_REPOSITORY are required.");
	return async (path, { json, ...options } = {}) => {
		const response = await request(`${env.GITHUB_API_URL ?? "https://api.github.com"}${path}`, {
			...options,
			signal: AbortSignal.timeout(20_000),
			headers: {
				Authorization: `Bearer ${env.GITHUB_TOKEN}`,
				Accept: "application/vnd.github+json",
				"X-GitHub-Api-Version": "2022-11-28",
				...(json ? { "Content-Type": "application/json" } : {})
			},
			...(json ? { body: JSON.stringify(json) } : {})
		});
		if (!response.ok)
			throw new Error(
				`GitHub request failed (${response.status}): ${options.method ?? "GET"} ${path}`
			);
		return response.status === 204 ? null : response.json();
	};
}
