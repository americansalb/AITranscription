import { useMemo } from "react";

interface ComparisonViewProps {
  rawText: string;
  polishedText: string;
  onClose: () => void;
}

interface DiffSegment {
  type: "same" | "removed" | "added";
  text: string;
}

// Simple word-level diff algorithm
function computeDiff(oldText: string, newText: string): DiffSegment[] {
  const oldWords = oldText.split(/(\s+)/);
  const newWords = newText.split(/(\s+)/);
  const segments: DiffSegment[] = [];

  // Simple LCS-based diff
  const matrix: number[][] = [];
  for (let i = 0; i <= oldWords.length; i++) {
    matrix[i] = [];
    for (let j = 0; j <= newWords.length; j++) {
      if (i === 0 || j === 0) {
        matrix[i][j] = 0;
      } else if (oldWords[i - 1] === newWords[j - 1]) {
        matrix[i][j] = matrix[i - 1][j - 1] + 1;
      } else {
        matrix[i][j] = Math.max(matrix[i - 1][j], matrix[i][j - 1]);
      }
    }
  }

  // Backtrack to find diff
  let i = oldWords.length;
  let j = newWords.length;
  const result: DiffSegment[] = [];

  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldWords[i - 1] === newWords[j - 1]) {
      result.unshift({ type: "same", text: oldWords[i - 1] });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || matrix[i][j - 1] >= matrix[i - 1][j])) {
      result.unshift({ type: "added", text: newWords[j - 1] });
      j--;
    } else {
      result.unshift({ type: "removed", text: oldWords[i - 1] });
      i--;
    }
  }

  // Merge consecutive segments of same type
  for (const segment of result) {
    if (segments.length > 0 && segments[segments.length - 1].type === segment.type) {
      segments[segments.length - 1].text += segment.text;
    } else {
      segments.push({ ...segment });
    }
  }

  return segments;
}

export function ComparisonView({ rawText, polishedText, onClose }: ComparisonViewProps) {
  const diff = useMemo(() => computeDiff(rawText, polishedText), [rawText, polishedText]);

  const stats = useMemo(() => {
    const removed = diff.filter(s => s.type === "removed").reduce((acc, s) => acc + s.text.trim().split(/\s+/).filter(Boolean).length, 0);
    const added = diff.filter(s => s.type === "added").reduce((acc, s) => acc + s.text.trim().split(/\s+/).filter(Boolean).length, 0);
    const same = diff.filter(s => s.type === "same").reduce((acc, s) => acc + s.text.trim().split(/\s+/).filter(Boolean).length, 0);
    return { removed, added, same, total: removed + added + same };
  }, [diff]);

  return (
    <div className="comparison-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="comparison-modal">
        <div className="comparison-header">
          <h2>Compare Raw vs Polished</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="comparison-stats">
          <div className="stat-pill removed">
            <span className="stat-count">−{stats.removed}</span>
            <span className="stat-label">removed</span>
          </div>
          <div className="stat-pill added">
            <span className="stat-count">+{stats.added}</span>
            <span className="stat-label">added</span>
          </div>
          <div className="stat-pill same">
            <span className="stat-count">{stats.same}</span>
            <span className="stat-label">unchanged</span>
          </div>
        </div>

        <div className="comparison-content">
          <div className="comparison-panel">
            <div className="panel-header">
              <span className="panel-label">Raw Transcription</span>
              <span className="panel-badge raw">Original</span>
            </div>
            <div className="panel-text">
              {rawText || <span className="empty">No raw text</span>}
            </div>
          </div>

          <div className="comparison-divider">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M5 12h14" />
              <path d="m12 5 7 7-7 7" />
            </svg>
          </div>

          <div className="comparison-panel">
            <div className="panel-header">
              <span className="panel-label">Polished Text</span>
              <span className="panel-badge polished">AI Enhanced</span>
            </div>
            <div className="panel-text">
              {polishedText || <span className="empty">No polished text</span>}
            </div>
          </div>
        </div>

        <div className="comparison-diff">
          <div className="diff-header">
            <span className="diff-label">Changes Highlighted</span>
            <div className="diff-legend">
              <span className="legend-item removed">Removed</span>
              <span className="legend-item added">Added</span>
            </div>
          </div>
          <div className="diff-content">
            {diff.map((segment, index) => (
              <span
                key={index}
                className={`diff-segment ${segment.type}`}
              >
                {segment.text}
              </span>
            ))}
          </div>
        </div>

        <div className="comparison-footer">
          <button className="comparison-close-btn" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </div>
  );
}
