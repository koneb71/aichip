import { describe, expect, it } from "vitest";
import { EngineCapabilities, EngineDescriptor, permissionBlocker, toolsBlocker } from "./engines";

const all: EngineCapabilities = {
  interactive_permissions: true,
  structured_rate_limit: true,
  resume_sessions: true,
  append_system_prompt: true,
  fixed_model_catalog: true,
  reports_cost: true,
  enforces_denied_tools: true,
  mcp_tools: true,
  auto_edit: true,
};

const engine = (id: string, label: string, caps: Partial<EngineCapabilities> = {}): EngineDescriptor => ({
  id,
  label,
  version: "1",
  authenticated: true,
  providers: [],
  capabilities: { ...all, ...caps },
});

const claude = engine("claude-code", "Claude Code");
const qwen = engine("qwen", "Qwen Code", { interactive_permissions: false });
const cursor = engine("cursor", "Cursor CLI", {
  interactive_permissions: false,
  mcp_tools: false,
  auto_edit: false,
});

describe("permissionBlocker", () => {
  it("refuses Auto-edit on an engine with no edit-only mode, and nothing else", () => {
    expect(permissionBlocker(cursor, "auto_edit")).toMatch(/Cursor CLI has no setting/);
    expect(permissionBlocker(cursor, "full_auto")).toBeNull();
    expect(permissionBlocker(qwen, "auto_edit")).toBeNull();
    expect(permissionBlocker(qwen, "reviewed")).toMatch(/can't stop to ask/);
  });
});

describe("toolsBlocker", () => {
  it("names the installed engines that can carry the tools", () => {
    const said = toolsBlocker(cursor, "the assistant", [claude, cursor, qwen]);
    expect(said).toMatch(/^Cursor CLI can't be handed Eren's tools/);
    expect(said).toMatch(/Pick Claude Code, Qwen Code\.$/);
  });

  it("says nothing for an engine that can, or one it does not know", () => {
    expect(toolsBlocker(qwen, "a team", [qwen])).toBeNull();
    expect(toolsBlocker(undefined, "a team", [cursor])).toBeNull();
  });

  it("offers no advice when no installed engine can", () => {
    expect(toolsBlocker(cursor, "a team", [cursor])).toMatch(/works through them\.$/);
  });
});
