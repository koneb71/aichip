import { describe, expect, it } from "vitest";
import { eventStep, lastActivity } from "./RunStream";
import type { StreamEvent } from "../lib/ws";

/** Shapes copied from a real org run's websocket frames. */
const ev = (over: Partial<StreamEvent>): StreamEvent =>
  ({ runId: "r", seq: 1, ts: "2026-07-29T00:00:00Z", type: "assistant_text", ...over }) as StreamEvent;

describe("eventStep", () => {
  it("reads the id the websocket actually sends", () => {
    // Replay frames carry `step_id`; the hook lifts it onto the event. A
    // regression here silently un-attributes every event in a team run.
    expect(eventStep(ev({ step_id: "s1" }))).toBe("s1");
    expect(eventStep(ev({ stepId: "s2" }))).toBe("s2");
    expect(eventStep(ev({}))).toBeUndefined();
  });
});

describe("lastActivity", () => {
  it("names the tool and its most telling argument", () => {
    const events = [
      ev({ type: "run_started", seq: 0 }),
      ev({ type: "tool_call", tool_name: "Bash", input: { command: "docker compose up -d" }, seq: 1 }),
    ];
    expect(lastActivity(events)).toBe("Bash docker compose up -d");
  });

  it("prefers a file path when there is no command", () => {
    const events = [ev({ type: "tool_call", tool_name: "Read", input: { file_path: "backend/app/main.py" } })];
    expect(lastActivity(events)).toBe("Read backend/app/main.py");
  });

  it("falls back to the most recent thing the agent said", () => {
    const events = [
      ev({ type: "tool_call", tool_name: "Bash", input: { command: "ls" }, seq: 1 }),
      ev({ type: "assistant_text", text: "  Wiring   the health endpoint\nnow ", seq: 2 }),
    ];
    // Collapsed to one line — this renders in a single-line chip.
    expect(lastActivity(events)).toBe("Wiring the health endpoint now");
  });

  it("skips event types that say nothing a person can act on", () => {
    // usage_updated and tool_result arrive constantly; neither answers
    // "what is it doing?", so the scan must look past them.
    const events = [
      ev({ type: "tool_call", tool_name: "Grep", input: { pattern: "health" }, seq: 1 }),
      ev({ type: "tool_result", is_error: false, summary: "3 matches", seq: 2 }),
      ev({ type: "usage_updated", seq: 3 }),
    ];
    expect(lastActivity(events)).toBe("Grep health");
  });

  it("surfaces a permission prompt as the thing blocking progress", () => {
    const events = [ev({ type: "permission_requested", tool_name: "Bash" })];
    expect(lastActivity(events)).toBe("waiting on you: Bash");
  });

  it("reports terminal states plainly", () => {
    expect(lastActivity([ev({ type: "run_completed" })])).toBe("finished");
    expect(lastActivity([ev({ type: "run_failed", reason: "worktree missing" })])).toBe(
      "failed: worktree missing",
    );
  });

  it("returns null when there is genuinely nothing to report", () => {
    expect(lastActivity([])).toBeNull();
    expect(lastActivity([ev({ type: "usage_updated" })])).toBeNull();
  });

  it("attributes activity to one teammate in a shared run", () => {
    // The whole point of step attribution: two specialists working at once
    // must not report each other's actions.
    const events = [
      ev({ type: "tool_call", tool_name: "Write", input: { file_path: "backend/health.py" }, step_id: "be", seq: 1 }),
      ev({ type: "tool_call", tool_name: "Write", input: { file_path: "frontend/Badge.tsx" }, step_id: "fe", seq: 2 }),
    ];
    expect(lastActivity(events, "be")).toBe("Write backend/health.py");
    expect(lastActivity(events, "fe")).toBe("Write frontend/Badge.tsx");
    expect(lastActivity(events, "docs")).toBeNull();
  });

  it("clips a long argument rather than letting it break the layout", () => {
    const long = "x".repeat(300);
    const out = lastActivity([ev({ type: "tool_call", tool_name: "Bash", input: { command: long } })])!;
    expect(out.length).toBeLessThan(80);
    expect(out.endsWith("…")).toBe(true);
  });
});
