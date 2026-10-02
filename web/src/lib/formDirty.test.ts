import { describe, expect, it } from "vitest";
import { formChanged } from "./formDirty";

describe("formChanged", () => {
  it("is clean when every field matches", () => {
    expect(formChanged({ name: "a", tier: "medium" }, { name: "a", tier: "medium" })).toBe(false);
  });
  it("sees an edit to any field", () => {
    expect(formChanged({ name: "a", prompt: "" }, { name: "a", prompt: "x" })).toBe(true);
  });
  it("treats every spelling of nothing as the same", () => {
    expect(formChanged({ reportsTo: null, role: undefined }, { reportsTo: "", role: "" })).toBe(false);
    expect(formChanged({ engine: null }, { engine: "claude-code" })).toBe(true);
  });
  it("compares lists by contents and order", () => {
    const one = { agent_id: "1" };
    const two = { agent_id: "2" };
    expect(formChanged({ members: [one, two] }, { members: [{ agent_id: "1" }, { agent_id: "2" }] })).toBe(false);
    expect(formChanged({ members: [one, two] }, { members: [two, one] })).toBe(true);
    expect(formChanged({ members: [one] }, { members: [one, two] })).toBe(true);
  });
  it("compares nested objects by value, missing keys as empty", () => {
    expect(formChanged({ m: { agent_id: "1" } }, { m: { agent_id: "1", role: "" } })).toBe(false);
    expect(formChanged({ m: { agent_id: "1" } }, { m: { agent_id: "1", role: "lead" } })).toBe(true);
  });
  it("does not mistake 0 or false for nothing", () => {
    expect(formChanged({ n: null }, { n: 0 })).toBe(true);
    expect(formChanged({ b: false }, { b: undefined })).toBe(true);
  });
});
