// HealthPill — Slice 9 resilience-stack JOIN UI (spec §12.4).
// Polls get_resilience_status every 10s; renders 🟢/🟡/🔴 + expandable
// 4-pillar detail.

import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';

type LayerStatus = { ok: boolean; label: string; detail: string };
type ResilienceStatus = {
  roll_up: 'green' | 'warn' | 'bad';
  pillars_ok: number;
  layer1: LayerStatus;
  layer2: LayerStatus;
  layer3: LayerStatus;
  layer4: LayerStatus;
};

const ROLL_UP_ICON: Record<ResilienceStatus['roll_up'], string> = {
  green: '🟢',
  warn: '🟡',
  bad: '🔴',
};
const ROLL_UP_LABEL: Record<ResilienceStatus['roll_up'], string> = {
  green: 'Stack OK',
  warn: '1 issue',
  bad: 'Stack degraded',
};

export function HealthPill({ projectDir }: { projectDir: string | null }) {
  const [status, setStatus] = useState<ResilienceStatus | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    if (!projectDir) return;
    let cancelled = false;
    const poll = async () => {
      try {
        const s = await invoke<ResilienceStatus>('get_resilience_status', { dir: projectDir });
        if (!cancelled) setStatus(s);
      } catch (e) {
        if (!cancelled) console.warn('[HealthPill] get_resilience_status failed:', e);
      }
    };
    void poll();
    const id = setInterval(poll, 10_000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, [projectDir]);

  if (!status) {
    return (
      <span className="protocol-panel__pill" style={{ background: '#fafbfc', color: '#5b6478' }}>
        ⏳ Stack…
      </span>
    );
  }

  const cls =
    status.roll_up === 'green' ? 'health-pill--green' :
    status.roll_up === 'warn' ? 'health-pill--warn' : 'health-pill--bad';

  return (
    <>
      <button
        type="button"
        className={`protocol-panel__pill health-pill ${cls}`}
        onClick={() => setExpanded(!expanded)}
        aria-expanded={expanded}
        title="Resilience stack health — click for layer details"
      >
        {ROLL_UP_ICON[status.roll_up]} {ROLL_UP_LABEL[status.roll_up]}
      </button>
      {expanded && (
        <div className="health-pill__detail" role="region" aria-label="Resilience layer details">
          {[status.layer1, status.layer2, status.layer3, status.layer4].map((layer, i) => (
            <div key={i} className={`health-pill__layer health-pill__layer--${layer.ok ? 'ok' : 'fail'}`}>
              <span className="health-pill__layer-icon">{layer.ok ? '✓' : '✗'}</span>
              <span className="health-pill__layer-label">{layer.label}</span>
              <span className="health-pill__layer-detail">{layer.detail}</span>
            </div>
          ))}
        </div>
      )}
    </>
  );
}
