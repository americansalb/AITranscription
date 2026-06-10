// UI2 root — "One Window" surface (decree msg 210). Single webview, one
// column of signal: relay + decisions + liveness. Everything else collapsed.
import { useEffect, useState } from "react";
import { useUi2Store } from "./store/store";
import { TopStrip } from "./components/TopStrip";
import { SignalFeed } from "./components/SignalFeed";
import { DecisionDock } from "./components/DecisionDock";
import { Composer } from "./components/Composer";
import { EngineRoom } from "./components/EngineRoom";
// Bundled faces (SIL OFL) — offline from a clean clone; tokens.css declares
// the stacks with system fallbacks. Weights per token sheet §3: 400/500/600.
import "@fontsource/space-grotesk/600.css";
import "@fontsource/inter/400.css";
import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "./tokens.css";
import "./ui2.css";

function bootstrapDir(): string {
  // Bootstrap hint only — the source of truth is the .vaak dir itself (§3.4).
  try {
    const raw = localStorage.getItem("vaak_collab_project_dir");
    if (!raw) return "";
    const parsed = JSON.parse(raw);
    return typeof parsed === "string" ? parsed : "";
  } catch {
    return "";
  }
}

export default function Ui2App() {
  const connect = useUi2Store((s) => s.connect);
  const projectDir = useUi2Store((s) => s.projectDir);
  const error = useUi2Store((s) => s.error);
  const [dirInput, setDirInput] = useState("");

  useEffect(() => {
    const dir = bootstrapDir();
    if (dir) void connect(dir);
  }, [connect]);

  if (!projectDir) {
    return (
      <div className="ui2 ui2-connect">
        <h1>Vaak</h1>
        <p>Open a project directory to start.</p>
        <form
          onSubmit={(e) => {
            e.preventDefault();
            if (dirInput.trim()) void connect(dirInput.trim());
          }}
        >
          <input
            value={dirInput}
            onChange={(e) => setDirInput(e.target.value)}
            placeholder="C:\path\to\project"
            aria-label="Project directory path"
          />
          <button type="submit">Open project</button>
        </form>
        {error && <p className="ui2-error">{error}</p>}
      </div>
    );
  }

  return (
    <div className="ui2 ui2-shell">
      <TopStrip />
      <main className="ui2-main">
        <SignalFeed />
        <DecisionDock />
      </main>
      <Composer />
      <EngineRoom />
      {error && (
        <div className="ui2-error ui2-error-bar" role="alert">
          {error}
        </div>
      )}
    </div>
  );
}
