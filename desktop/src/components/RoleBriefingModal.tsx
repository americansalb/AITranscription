import { useEffect, useState } from "react";

interface RoleBriefingModalProps {
  projectDir: string;
  roleSlug: string;
  roleTitle: string;
  roleColor: string;
  onClose: () => void;
}

export function RoleBriefingModal({ projectDir, roleSlug, roleTitle, roleColor, onClose }: RoleBriefingModalProps) {
  const [content, setContent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    (async () => {
      try {
        if (window.__TAURI__) {
          const { invoke } = await import("@tauri-apps/api/core");
          const result = await invoke<string>("read_role_briefing", { dir: projectDir, roleSlug });
          setContent(result);
        }
      } catch (e) {
        setError(String(e));
      }
    })();
  }, [projectDir, roleSlug]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  return (
    <div className="briefing-overlay" onClick={handleBackdropClick}>
      <div className="briefing-modal">
        <div className="briefing-header" style={{ borderBottomColor: roleColor }}>
          <div className="briefing-header-left">
            <span className="briefing-role-dot" style={{ background: roleColor }} />
            <h2 className="briefing-title">{roleTitle}</h2>
            <span className="briefing-slug">{roleSlug}</span>
          </div>
          <button className="briefing-close-btn" onClick={onClose}>
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M1 1L13 13M13 1L1 13" stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
            </svg>
          </button>
        </div>
        <div className="briefing-body">
          {error ? (
            <div className="briefing-error">
              No briefing file found for this role. Create <code>.vaak/roles/{roleSlug}.md</code> to add one.
            </div>
          ) : content === null ? (
            <div className="briefing-loading">Loading briefing...</div>
          ) : (
            <div className="briefing-content">
              {renderMarkdown(content)}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Simple markdown renderer — handles headers, bold, code, bullets, and paragraphs */
function renderMarkdown(md: string) {
  const lines = md.split("\n");
  const elements: React.ReactNode[] = [];
  let key = 0;
  let listItems: React.ReactNode[] = [];

  const flushList = () => {
    if (listItems.length > 0) {
      elements.push(<ul key={key++}>{listItems}</ul>);
      listItems = [];
    }
  };

  for (const line of lines) {
    // Headers
    if (line.startsWith("### ")) {
      flushList();
      elements.push(<h4 key={key++}>{inlineFormat(line.slice(4))}</h4>);
    } else if (line.startsWith("## ")) {
      flushList();
      elements.push(<h3 key={key++}>{inlineFormat(line.slice(3))}</h3>);
    } else if (line.startsWith("# ")) {
      flushList();
      // Skip the top-level heading since we show the title in the header
    } else if (line.match(/^[-*] /)) {
      // Bullet list item
      listItems.push(<li key={key++}>{inlineFormat(line.slice(2))}</li>);
    } else if (line.match(/^\d+\. /)) {
      // Numbered list — render as bullet for simplicity
      const text = line.replace(/^\d+\.\s*/, "");
      listItems.push(<li key={key++}>{inlineFormat(text)}</li>);
    } else if (line.trim() === "") {
      flushList();
    } else {
      flushList();
      elements.push(<p key={key++}>{inlineFormat(line)}</p>);
    }
  }
  flushList();
  return elements;
}

/** Handle inline formatting: **bold**, `code`, and plain text */
function inlineFormat(text: string): React.ReactNode {
  const parts: React.ReactNode[] = [];
  let remaining = text;
  let key = 0;

  while (remaining.length > 0) {
    // Bold
    const boldMatch = remaining.match(/\*\*(.+?)\*\*/);
    // Code
    const codeMatch = remaining.match(/`(.+?)`/);

    // Find the earliest match
    const boldIdx = boldMatch ? remaining.indexOf(boldMatch[0]) : Infinity;
    const codeIdx = codeMatch ? remaining.indexOf(codeMatch[0]) : Infinity;

    if (boldIdx === Infinity && codeIdx === Infinity) {
      parts.push(remaining);
      break;
    }

    if (boldIdx <= codeIdx && boldMatch) {
      if (boldIdx > 0) parts.push(remaining.slice(0, boldIdx));
      parts.push(<strong key={key++}>{boldMatch[1]}</strong>);
      remaining = remaining.slice(boldIdx + boldMatch[0].length);
    } else if (codeMatch) {
      if (codeIdx > 0) parts.push(remaining.slice(0, codeIdx));
      parts.push(<code key={key++}>{codeMatch[1]}</code>);
      remaining = remaining.slice(codeIdx + codeMatch[0].length);
    }
  }

  return parts.length === 1 ? parts[0] : <>{parts}</>;
}
