// Audit: every invoke("X", ...) call site in the frontend must have a matching
// X in the Tauri `generate_handler![...]` list at main.rs. This test catches
// the exact class of bug that shipped in 06b6cee — StartSequenceModal calling
// invoke("discussion_control") against a Tauri command that was never
// registered. Unit tests that mock invoke() pass regardless; only a parity
// check at the string-level catches UI-to-Tauri wiring drift.

import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

const FRONTEND_ROOT = resolve(__dirname, "..");
const MAIN_RS = resolve(__dirname, "../../src-tauri/src/main.rs");

// Recursively collect .ts / .tsx files under the frontend source tree,
// excluding test files (which intentionally use mock invoke names) and
// node_modules.
function walkTsFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "__tests__" || entry === "mocks") continue;
    const full = join(dir, entry);
    const st = statSync(full);
    if (st.isDirectory()) {
      walkTsFiles(full, out);
    } else if (entry.endsWith(".ts") || entry.endsWith(".tsx")) {
      out.push(full);
    }
  }
  return out;
}

interface InvokeSite {
  name: string;
  file: string;
  line: number;
}

// Extract every invoke("X", ...) call site. Skip lines that are full-line
// comments or that contain the pattern inside a string literal preceded by
// common error-message markers. Conservative false-positive avoidance.
function extractInvokeSites(files: string[]): InvokeSite[] {
  const sites: InvokeSite[] = [];
  const pattern = /(?:^|[\s(.])invoke\s*\(\s*["']([a-z_][a-z0-9_]*)["']/g;
  for (const file of files) {
    const lines = readFileSync(file, "utf8").split(/\r?\n/);
    for (let i = 0; i < lines.length; i++) {
      const raw = lines[i];
      const trimmed = raw.replace(/^\s*/, "");
      if (trimmed.startsWith("//") || trimmed.startsWith("*")) continue;
      const re = new RegExp(pattern.source, "g");
      let m: RegExpExecArray | null;
      while ((m = re.exec(raw)) !== null) {
        sites.push({ name: m[1], file, line: i + 1 });
      }
    }
  }
  return sites;
}

// Parse `tauri::generate_handler![ ... ]` from main.rs and collect every
// registered Tauri command name. Module-prefixed entries (queue::foo,
// launcher::bar) contribute their leaf name since that is what the frontend
// invokes.
function extractRegisteredCommands(mainRsPath: string): Set<string> {
  const src = readFileSync(mainRsPath, "utf8");
  const marker = "generate_handler![";
  const start = src.indexOf(marker);
  if (start < 0) {
    throw new Error(`Could not locate ${marker} in ${mainRsPath}`);
  }
  // Balance the brackets to find the matching ].
  let depth = 0;
  let end = -1;
  for (let i = start + marker.length; i < src.length; i++) {
    const c = src[i];
    if (c === "[") depth++;
    else if (c === "]") {
      if (depth === 0) { end = i; break; }
      depth--;
    }
  }
  if (end < 0) {
    throw new Error(`Could not find closing ] for ${marker} in ${mainRsPath}`);
  }
  const body = src
    .slice(start + marker.length, end)
    .replace(/\/\/.*$/gm, ""); // strip line comments
  const names = body
    .split(",")
    .map(s => s.trim())
    .filter(Boolean)
    .map(s => (s.includes("::") ? s.split("::").pop()! : s));
  return new Set(names);
}

describe("tauri-command parity (frontend invoke → registered Rust command)", () => {
  it("every invoke(name) in desktop/src is a registered Tauri command in main.rs", () => {
    const files = walkTsFiles(FRONTEND_ROOT);
    const sites = extractInvokeSites(files);
    const registered = extractRegisteredCommands(MAIN_RS);

    const unregistered = sites.filter(s => !registered.has(s.name));

    if (unregistered.length > 0) {
      const repoRoot = resolve(FRONTEND_ROOT, "..");
      const summary = unregistered
        .map(s => {
          const rel = s.file.replace(repoRoot, "").replace(/\\/g, "/").replace(/^\/+/, "");
          return `  • invoke("${s.name}") at ${rel}:${s.line}`;
        })
        .join("\n");
      const distinct = Array.from(new Set(unregistered.map(s => s.name))).sort();
      throw new Error(
        `Found ${unregistered.length} invoke() call site(s) referencing ${distinct.length} command(s) not registered in main.rs's tauri::generate_handler![...].\n\n` +
          `Missing commands (must be added to the handler list):\n` +
          distinct.map(n => `  - ${n}`).join("\n") +
          `\n\nCall sites:\n${summary}\n\n` +
          `Each unregistered name is a runtime "Command not found" error waiting for the user. ` +
          `Either add a #[tauri::command] fn <name>(...) to main.rs and register it in generate_handler!, ` +
          `or change the call site to invoke a correct name.`
      );
    }

    expect(unregistered).toEqual([]);
  });
});
