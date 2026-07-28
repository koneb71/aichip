import { describe, expect, it } from "vitest";
import { annotateDiff, hunkText, isCommentable } from "./diff";

const DIFF = `diff --git a/src/app.py b/src/app.py
index 1234567..89abcde 100644
--- a/src/app.py
+++ b/src/app.py
@@ -10,6 +10,8 @@ def load():
     conn = connect()
-    return None
+    if conn is None:
+        raise RuntimeError("no db")
+    return conn
     # trailing context
diff --git a/README.md b/README.md
--- a/README.md
+++ b/README.md
@@ -1,2 +1,3 @@
 # Title
+A new line.
`;

describe("annotateDiff", () => {
  const lines = annotateDiff(DIFF);
  const byText = (needle: string) => lines.find((l) => l.text.includes(needle))!;

  it("attributes lines to the file they belong to", () => {
    expect(byText("raise RuntimeError").file).toBe("src/app.py");
    expect(byText("A new line.").file).toBe("README.md");
  });

  it("does not mistake the +++ header for an added line", () => {
    // The header starts with '+', which a naive check reads as an addition —
    // and it would then consume a line number that belongs to real code.
    expect(byText("+++ b/src/app.py").kind).toBe("meta");
  });

  it("numbers lines against the new file", () => {
    // Hunk starts at 10: context 'conn = connect()' is 10, the deletion takes
    // no number, then the three added lines are 11, 12, 13.
    expect(byText("conn = connect()").newLine).toBe(10);
    expect(byText("if conn is None").newLine).toBe(11);
    expect(byText('raise RuntimeError("no db")').newLine).toBe(12);
    expect(byText("return conn").newLine).toBe(13);
    expect(byText("# trailing context").newLine).toBe(14);
  });

  it("gives deletions no line number and does not let them advance the count", () => {
    expect(byText("-    return None").newLine).toBeNull();
    expect(byText("-    return None").kind).toBe("del");
  });

  it("restarts numbering at each hunk header", () => {
    expect(byText("# Title").newLine).toBe(1);
    expect(byText("A new line.").newLine).toBe(2);
  });

  it("groups lines into hunks", () => {
    expect(byText("return conn").hunk).toBe(0);
    expect(byText("A new line.").hunk).toBe(1);
  });

  it("survives a malformed hunk header without throwing", () => {
    const lines = annotateDiff("+++ b/x.txt\n@@ garbage @@\n context");
    expect(lines.at(-1)!.file).toBe("x.txt");
  });

  it("treats a new file's /dev/null source as having no path yet", () => {
    const lines = annotateDiff("--- /dev/null\n+++ /dev/null\n");
    expect(lines[1].file).toBeNull();
  });
});

describe("hunkText", () => {
  it("returns just that hunk's body, without file headers", () => {
    const text = hunkText(annotateDiff(DIFF), 0);
    expect(text).toContain("return conn");
    expect(text).not.toContain("+++ b/src/app.py");
    expect(text).not.toContain("A new line.");
  });

  it("caps runaway hunks", () => {
    const huge = "+++ b/a\n@@ -1,1 +1,900 @@\n" + "+x\n".repeat(900);
    expect(hunkText(annotateDiff(huge), 0).split("\n").length).toBeLessThanOrEqual(40);
  });
});

describe("isCommentable", () => {
  it("accepts code and refuses metadata", () => {
    const lines = annotateDiff(DIFF);
    expect(isCommentable(lines.find((l) => l.text.includes("return conn"))!)).toBe(true);
    expect(isCommentable(lines.find((l) => l.text.startsWith("@@"))!)).toBe(false);
    expect(isCommentable(lines.find((l) => l.text.startsWith("index "))!)).toBe(false);
  });
});
