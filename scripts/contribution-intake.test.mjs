import test from "node:test";
import assert from "node:assert/strict";
import { advise, isFeature, issueNumbers } from "./contribution-intake.mjs";

const repo = "owner/bloom";
const pr = overrides => ({
  repository: { full_name: repo },
  pull_request: { number: 4, title: "feat: new panel", body: "Closes #12",
    labels: [], user: { type: "User" }, ...overrides }
});
const mock = ({ accepted = false, comments = [] } = {}) => {
  const writes = [];
  const request = async (path, method = "GET", data) => {
    if (method !== "GET") { writes.push({ path, method, data }); return {}; }
    if (path === "/repos/owner/bloom/issues/12")
      return { labels: accepted ? [{ name: "accepted" }] : [] };
    if (path.includes("/comments?")) return comments;
    throw new Error("Unexpected read: " + path);
  };
  return { request, writes };
};

test("small direct fixes need no issue approval", async () => {
  const m = mock();
  await advise(pr({ title: "fix: restore a window", body: "Small reproduced bug" }), m.request);
  assert.deepEqual(m.writes, []);
});

test("feature detection accepts template and conventional title, not unchecked boxes", () => {
  assert.ok(isFeature({ title: "feat(dock): panel" }));
  assert.ok(isFeature({ body: "- [x] New feature or broader behavior change" }));
  assert.equal(isFeature({ title: "fix: window", body: "- [ ] New feature or broader behavior change" }), false);
});

test("only local and same-repository issue references count", () => {
  assert.deepEqual(issueNumbers("Closes #12; https://github.com/owner/bloom/issues/13; " +
    "other/repo#14; https://github.com/other/repo/issues/15", repo), [13, 12]);
});

test("unapproved feature gets an advisory, never approval or closure", async () => {
  const m = mock();
  await advise(pr({}), m.request);
  assert.equal(m.writes.length, 2);
  assert.deepEqual(m.writes[0].data, { labels: ["needs-discussion"] });
  assert.match(m.writes[1].data.body, /advisory reminder/);
  assert.ok(m.writes.every(w => !("state" in w.data)));
});

test("accepted issue resolves the advisory label without implying merge approval", async () => {
  const m = mock({ accepted: true, comments: [{
    id: 99, user: { login: "github-actions[bot]", type: "Bot" },
    body: "<!-- bloom-contribution-intake --> old reminder"
  }] });
  await advise(pr({ labels: [{ name: "needs-discussion" }] }), m.request);
  assert.equal(m.writes[0].method, "DELETE");
  assert.equal(m.writes[1].path, "/repos/owner/bloom/issues/comments/99");
  assert.match(m.writes[1].data.body, /not merge approval/);
});

test("never edits a contributor comment with a copied marker", async () => {
  const m = mock({ comments: [{
    id: 88, user: { login: "contributor", type: "User" },
    body: "<!-- bloom-contribution-intake -->"
  }] });
  await advise(pr({}), m.request);
  assert.equal(m.writes[1].method, "POST");
});

test("repeated intake updates one reminder rather than adding comments", async () => {
  const first = mock();
  await advise(pr({}), first.request);
  const body = first.writes[1].data.body;
  const again = mock({ comments: [{
    id: 99, user: { login: "github-actions[bot]", type: "Bot" }, body
  }] });
  await advise(pr({}), again.request);
  assert.equal(again.writes.length, 1);
  assert.ok(again.writes.every(w => !w.path.includes("/comments")));
});

test("bugs and bot dependency updates remain welcome; feature issues need discussion", async () => {
  const bug = mock();
  await advise({ repository: { full_name: repo }, issue: {
    number: 1, title: "[Bug]: crash", labels: [{ name: "bug" }]
  } }, bug.request);
  assert.deepEqual(bug.writes, []);
  const feature = mock();
  await advise({ repository: { full_name: repo }, issue: {
    number: 2, title: "[Feature]: panel", labels: [{ name: "enhancement" }]
  } }, feature.request);
  assert.deepEqual(feature.writes[0].data, { labels: ["needs-discussion"] });
  const bot = mock();
  await advise(pr({ user: { type: "Bot" } }), bot.request);
  assert.deepEqual(bot.writes, []);
});
