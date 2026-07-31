import { describe, expect, it } from "vitest";
import { languageFor } from "./language";

describe("languageFor", () => {
  it("covers the languages people actually have open", () => {
    expect(languageFor("src/main.rs")).toBe("rust");
    expect(languageFor("cmd/server/main.go")).toBe("go");
    expect(languageFor("app/models.py")).toBe("python");
    expect(languageFor("web/src/App.tsx")).toBe("typescript");
    expect(languageFor("web/src/lib/api.ts")).toBe("typescript");
    expect(languageFor("compose.yaml")).toBe("yaml");
    expect(languageFor("Cargo.toml")).toBe("ini");
    expect(languageFor("schema.sql")).toBe("sql");
    expect(languageFor("main.tf")).toBe("hcl");
    expect(languageFor("README.md")).toBe("markdown");
    expect(languageFor("package.json")).toBe("json");
  });

  it("handles the files with no extension at all", () => {
    // The whole reason this is a map and not a `split(".").pop()`.
    expect(languageFor("Dockerfile")).toBe("dockerfile");
    expect(languageFor("deploy/Dockerfile")).toBe("dockerfile");
    expect(languageFor("Makefile")).toBe("makefile");
    expect(languageFor(".gitignore")).toBe("plaintext");
    expect(languageFor(".env")).toBe("shell");
  });

  it("reads a suffixed name from its leading part", () => {
    // `Dockerfile.dev` is the common one; without this it falls to the
    // extension table, finds "dev", and gives up.
    expect(languageFor("Dockerfile.dev")).toBe("dockerfile");
    expect(languageFor(".env.local")).toBe("shell");
  });

  it("takes the last extension when there are several", () => {
    expect(languageFor("docker-compose.prod.yml")).toBe("yaml");
    expect(languageFor("index.d.ts")).toBe("typescript");
  });

  it("is case-insensitive about the name", () => {
    expect(languageFor("SRC/MAIN.RS")).toBe("rust");
    expect(languageFor("dockerfile")).toBe("dockerfile");
  });

  it("falls back to plaintext rather than guessing", () => {
    // No colour beats wrong colour — a mislabelled file gets tokenised
    // against the wrong grammar and looks broken.
    expect(languageFor("data.bin")).toBe("plaintext");
    expect(languageFor("LICENSE")).toBe("plaintext");
    expect(languageFor("noextension")).toBe("plaintext");
    expect(languageFor("")).toBe("plaintext");
    expect(languageFor("weird.")).toBe("plaintext");
  });
});
