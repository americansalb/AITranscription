import { useState } from "react";

interface TranscriptEntry {
  id: string;
  rawText: string;
  polishedText: string;
  context: string;
  formality: string;
  timestamp: number;
  confidence?: number;
}

interface ExportModalProps {
  history: TranscriptEntry[];
  onClose: () => void;
}

type ExportFormat = "json" | "csv" | "txt";

export function ExportModal({ history, onClose }: ExportModalProps) {
  const [format, setFormat] = useState<ExportFormat>("json");
  const [includeRaw, setIncludeRaw] = useState(true);
  const [includePolished, setIncludePolished] = useState(true);
  const [includeMetadata, setIncludeMetadata] = useState(true);

  const handleExport = () => {
    let content: string;
    let filename: string;
    let mimeType: string;

    const filteredHistory = history.map(entry => {
      const result: Record<string, unknown> = {};
      if (includePolished) result.polishedText = entry.polishedText;
      if (includeRaw) result.rawText = entry.rawText;
      if (includeMetadata) {
        result.context = entry.context;
        result.formality = entry.formality;
        result.timestamp = new Date(entry.timestamp).toISOString();
        if (entry.confidence !== undefined) result.confidence = entry.confidence;
      }
      return result;
    });

    switch (format) {
      case "json":
        content = JSON.stringify(filteredHistory, null, 2);
        filename = `vaak-transcripts-${Date.now()}.json`;
        mimeType = "application/json";
        break;

      case "csv":
        const headers = [];
        if (includeMetadata) headers.push("Timestamp", "Context", "Formality", "Confidence");
        if (includeRaw) headers.push("Raw Text");
        if (includePolished) headers.push("Polished Text");

        const rows = filteredHistory.map(entry => {
          const row = [];
          if (includeMetadata) {
            row.push(
              entry.timestamp || "",
              entry.context || "",
              entry.formality || "",
              entry.confidence !== undefined ? entry.confidence : ""
            );
          }
          if (includeRaw) row.push(`"${(entry.rawText as string || "").replace(/"/g, '""')}"`);
          if (includePolished) row.push(`"${(entry.polishedText as string || "").replace(/"/g, '""')}"`);
          return row.join(",");
        });

        content = [headers.join(","), ...rows].join("\n");
        filename = `vaak-transcripts-${Date.now()}.csv`;
        mimeType = "text/csv";
        break;

      case "txt":
        content = filteredHistory.map((entry, index) => {
          const lines = [`--- Transcript ${index + 1} ---`];
          if (includeMetadata && entry.timestamp) lines.push(`Date: ${entry.timestamp}`);
          if (includeMetadata && entry.context) lines.push(`Context: ${entry.context}`);
          if (includeMetadata && entry.formality) lines.push(`Formality: ${entry.formality}`);
          if (includeMetadata && entry.confidence !== undefined) lines.push(`Confidence: ${(entry.confidence as number * 100).toFixed(1)}%`);
          if (includeRaw && entry.rawText) lines.push(`\nRaw:\n${entry.rawText}`);
          if (includePolished && entry.polishedText) lines.push(`\nPolished:\n${entry.polishedText}`);
          return lines.join("\n");
        }).join("\n\n");
        filename = `vaak-transcripts-${Date.now()}.txt`;
        mimeType = "text/plain";
        break;
    }

    // Download the file
    const blob = new Blob([content], { type: mimeType });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);

    onClose();
  };

  return (
    <div className="export-overlay" onClick={(e) => e.target === e.currentTarget && onClose()}>
      <div className="export-modal">
        <div className="export-header">
          <h2>Export Transcripts</h2>
          <button className="close-btn" onClick={onClose}>
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <line x1="18" y1="6" x2="6" y2="18" />
              <line x1="6" y1="6" x2="18" y2="18" />
            </svg>
          </button>
        </div>

        <div className="export-content">
          <div className="export-section">
            <h3>Format</h3>
            <div className="format-options">
              <label className={`format-option ${format === "json" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="format"
                  value="json"
                  checked={format === "json"}
                  onChange={() => setFormat("json")}
                />
                <div className="format-icon">📋</div>
                <span className="format-label">JSON</span>
                <span className="format-desc">Structured data</span>
              </label>
              <label className={`format-option ${format === "csv" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="format"
                  value="csv"
                  checked={format === "csv"}
                  onChange={() => setFormat("csv")}
                />
                <div className="format-icon">📊</div>
                <span className="format-label">CSV</span>
                <span className="format-desc">Spreadsheet</span>
              </label>
              <label className={`format-option ${format === "txt" ? "selected" : ""}`}>
                <input
                  type="radio"
                  name="format"
                  value="txt"
                  checked={format === "txt"}
                  onChange={() => setFormat("txt")}
                />
                <div className="format-icon">📝</div>
                <span className="format-label">Text</span>
                <span className="format-desc">Plain text</span>
              </label>
            </div>
          </div>

          <div className="export-section">
            <h3>Include</h3>
            <div className="include-options">
              <label className="include-option">
                <input
                  type="checkbox"
                  checked={includePolished}
                  onChange={(e) => setIncludePolished(e.target.checked)}
                />
                <span>Polished text</span>
              </label>
              <label className="include-option">
                <input
                  type="checkbox"
                  checked={includeRaw}
                  onChange={(e) => setIncludeRaw(e.target.checked)}
                />
                <span>Raw transcription</span>
              </label>
              <label className="include-option">
                <input
                  type="checkbox"
                  checked={includeMetadata}
                  onChange={(e) => setIncludeMetadata(e.target.checked)}
                />
                <span>Metadata (date, context, confidence)</span>
              </label>
            </div>
          </div>

          <div className="export-preview">
            <span className="preview-label">
              {history.length} transcript{history.length !== 1 ? "s" : ""} will be exported
            </span>
          </div>
        </div>

        <div className="export-footer">
          <button className="export-cancel-btn" onClick={onClose}>
            Cancel
          </button>
          <button
            className="export-download-btn"
            onClick={handleExport}
            disabled={!includeRaw && !includePolished}
          >
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2">
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" />
              <line x1="12" y1="15" x2="12" y2="3" />
            </svg>
            Download {format.toUpperCase()}
          </button>
        </div>
      </div>
    </div>
  );
}
