import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

const MARKER = "<!-- bloom-contribution-intake -->";

export function isFeature(item) {
  return /^feat(?:\([^)]*\))?!?:/i.test(item.title ?? "") ||
    /^\s*-\s*\[[xX]\]\s*New feature or broader behavior change\s*$/m.test(item.body ?? "") ||
    (item.labels ?? []).some(label => (typeof label === "string" ? label : label.name) === "enhancement");
}

export function issueNumbers(body, repo) {
  const result = new Set();
  // Explicit same-repository URLs and local references, never owner/repo#123.
  const escaped = repo.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (const match of body.matchAll(new RegExp("https://github\\.com/" + escaped + "/issues/(\\d+)", "gi")))
    result.add(Number(match[1]));
  const withoutUrls = body.replace(/https?:\/\/\S+/g, "");
  for (const match of withoutUrls.matchAll(/(?<![\w/])#([1-9]\d*)\b/g))
    result.add(Number(match[1]));
  return [...result].slice(0, 20);
}

export async function advise(event, request) {
  const repo = event.repository.full_name;
  const item = event.pull_request ?? event.issue;
  if (!item || item.user?.type === "Bot") return;
  const root = `/repos/${repo}`;
  const labelPath = `${root}/issues/${item.number}/labels`;
  if (!event.pull_request) {
    if (isFeature(item) && !(item.labels ?? []).some(label => label.name === "accepted"))
      await request(labelPath, "POST", { labels: ["needs-discussion"] });
    return;
  }

  let accepted = false;
  if (isFeature(item)) {
    for (const number of issueNumbers(item.body ?? "", repo)) {
      const issue = await request(`${root}/issues/${number}`);
      if (issue && !issue.pull_request && issue.labels.some(label => label.name === "accepted")) {
        accepted = true;
        break;
      }
    }
  }
  const needsDiscussion = isFeature(item) && !accepted;
  const labels = item.labels ?? [];
  if (needsDiscussion) {
    await request(labelPath, "POST", { labels: ["needs-discussion"] });
  }

  // Inspect only our own marked comments; never edit a contributor's comment.
  let own;
  for (let page = 1; ; page++) {
    const comments = await request(`${root}/issues/${item.number}/comments?per_page=100&page=${page}`);
    own = comments.find(comment => comment.user.type === "Bot" && comment.user.login === "github-actions[bot]" && comment.body.includes(MARKER));
    if (own || comments.length < 100) break;
  }
  if (!needsDiscussion && !own) return;
  if (!needsDiscussion && labels.some(label => label.name === "needs-discussion"))
    await request(`${labelPath}/needs-discussion`, "DELETE");
  const body = needsDiscussion
    ? `${MARKER}\nThanks for contributing. This looks like a feature or broader change; please link one focused issue whose scope a maintainer has agreed on (the accepted label). An open issue or green CI alone is not approval. Small bug fixes can go directly to a PR. A draft/prototype is welcome while discussing the approach; please wait for feedback before opening a batch of feature PRs.\n\nThis is an advisory reminder, not a failed check or automatic closure. See CONTRIBUTING.md. If prior maintainer agreement is missing its label, ask for it and edit the PR description to refresh this reminder.`
    : `${MARKER}\nThe scope reminder is resolved. This is not merge approval; code review and relevant validation still apply.`;
  if (own && own.body === body) return;
  await request(own ? `${root}/issues/comments/${own.id}` : `${root}/issues/${item.number}/comments`,
    own ? "PATCH" : "POST", { body });
}

async function main() {
  const event = JSON.parse(await readFile(process.env.GITHUB_EVENT_PATH, "utf8"));
  const api = process.env.GITHUB_API_URL ?? "https://api.github.com";
  const request = async (path, method = "GET", data) => {
    const response = await fetch(api + path, {
      method,
      headers: {
        Authorization: `Bearer ${process.env.GITHUB_TOKEN}`,
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
        ...(data ? { "Content-Type": "application/json" } : {})
      },
      ...(data ? { body: JSON.stringify(data) } : {})
    });
    if (response.status === 404 && method === "GET") return null;
    if (!response.ok) throw new Error(`GitHub ${method} ${path}: ${response.status}`);
    return response.status === 204 ? null : response.json();
  };
  await advise(event, request);
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
