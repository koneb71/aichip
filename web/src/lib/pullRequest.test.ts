import { describe, expect, it } from "vitest";
import type { TaskPullRequest } from "./api";
import { prSummary, prTone, shouldPoll, syncedLabel } from "./pullRequest";

const pr = (over: Partial<TaskPullRequest> = {}): TaskPullRequest => ({
  number: 12,
  url: "https://github.com/o/r/pull/12",
  state: "open",
  checks: "none",
  review: null,
  syncedAt: null,
  ...over,
});

describe("prSummary", () => {
  it("lets merged outrank everything, because it is the end of the story", () => {
    expect(prSummary(pr({ state: "merged", checks: "failing" }))).toBe("merged");
  });

  it("puts a failing check above an approval", () => {
    // Approved code that does not build is not ready, and the chip has room
    // for one thing.
    expect(prSummary(pr({ checks: "failing", review: "approved" }))).toBe("checks failing");
  });

  it("never calls a draft ready, however green it is", () => {
    expect(prSummary(pr({ state: "draft", checks: "passing", review: "approved" }))).toBe("draft");
  });

  it("does not read a missing review as a blocked one", () => {
    // A repository with no review rules reports nothing. Saying "changes
    // requested" there would invent a reviewer.
    expect(prSummary(pr({ review: null, checks: "passing" }))).toBe("open");
  });

  it("says checks are running rather than guessing the outcome", () => {
    expect(prSummary(pr({ checks: "pending" }))).toBe("checks running");
  });
});

describe("prTone", () => {
  it("colours by the same rule it labels by", () => {
    expect(prTone(pr({ checks: "failing" })).text).toContain("danger");
    expect(prTone(pr({ review: "approved" })).text).toContain("easy");
    expect(prTone(pr({ state: "merged" })).dot).toContain("complex");
  });
});

describe("syncedLabel", () => {
  const now = Date.UTC(2026, 7, 4, 12, 0, 0);
  const ago = (secs: number) => new Date(now - secs * 1000).toISOString();

  it("counts up in the units a person would use", () => {
    expect(syncedLabel(ago(5), now)).toBe("just now");
    expect(syncedLabel(ago(300), now)).toBe("5m ago");
    expect(syncedLabel(ago(7200), now)).toBe("2h ago");
    expect(syncedLabel(ago(172_800), now)).toBe("2d ago");
  });

  it("does not report a negative age when the clocks disagree", () => {
    // Server and browser clocks differ routinely; "synced in −3 minutes" is
    // not a thing that can be true.
    expect(syncedLabel(new Date(now + 180_000).toISOString(), now)).toBe("just now");
  });

  it("says it has never looked rather than implying it just did", () => {
    expect(syncedLabel(null, now)).toBe("never checked");
    expect(syncedLabel("not a date", now)).toBe("never checked");
  });
});

describe("shouldPoll", () => {
  it("only asks again while something is actually in flight", () => {
    expect(shouldPoll(pr({ state: "open", checks: "pending" }))).toBe(true);
    expect(shouldPoll(pr({ state: "draft", checks: "pending" }))).toBe(true);
  });

  it("stops once nothing can change", () => {
    // A merged pull request will never differ again, and a card nobody is
    // looking at should not cost a gh process per interval forever.
    expect(shouldPoll(pr({ state: "merged", checks: "pending" }))).toBe(false);
    expect(shouldPoll(pr({ state: "closed", checks: "pending" }))).toBe(false);
    expect(shouldPoll(pr({ state: "open", checks: "passing" }))).toBe(false);
    expect(shouldPoll(pr({ state: "open", checks: "none" }))).toBe(false);
    expect(shouldPoll(null)).toBe(false);
  });
});
