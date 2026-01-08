import { useState, useEffect } from 'react';
import {
  getLearningStats,
  getCorrections,
  getCorrectionRules,
  deleteCorrection,
  deleteCorrectionRule,
  addCorrectionRule,
  trainCorrectionModel,
  trainWhisperModel,
  type LearningStats,
  type Correction,
  type CorrectionRule,
} from '../lib/api';

interface LearningDashboardProps {
  apiUrl: string;
  token: string;
  onClose: () => void;
}

export function LearningDashboard({ onClose }: LearningDashboardProps) {
  const [stats, setStats] = useState<LearningStats | null>(null);
  const [corrections, setCorrections] = useState<Correction[]>([]);
  const [rules, setRules] = useState<CorrectionRule[]>([]);
  const [activeTab, setActiveTab] = useState<'overview' | 'corrections' | 'rules'>('overview');
  const [loading, setLoading] = useState(true);
  const [training, setTraining] = useState<'none' | 'correction' | 'whisper'>('none');
  const [error, setError] = useState<string | null>(null);

  // New rule form
  const [newRulePattern, setNewRulePattern] = useState('');
  const [newRuleReplacement, setNewRuleReplacement] = useState('');
  const [newRuleIsRegex, setNewRuleIsRegex] = useState(false);

  useEffect(() => {
    fetchData();
  }, []);

  async function fetchData() {
    setLoading(true);
    setError(null);
    try {
      const [statsData, correctionsData, rulesData] = await Promise.all([
        getLearningStats(),
        getCorrections(50),
        getCorrectionRules(),
      ]);

      setStats(statsData);
      setCorrections(correctionsData.corrections);
      setRules(rulesData.rules);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load learning data');
    } finally {
      setLoading(false);
    }
  }

  async function handleDeleteCorrection(id: number) {
    try {
      await deleteCorrection(id);
      setCorrections(corrections.filter(c => c.id !== id));
    } catch (err) {
      console.error('Failed to delete correction:', err);
    }
  }

  async function handleDeleteRule(id: number) {
    try {
      await deleteCorrectionRule(id);
      setRules(rules.filter(r => r.id !== id));
    } catch (err) {
      console.error('Failed to delete rule:', err);
    }
  }

  async function handleAddRule() {
    if (!newRulePattern || !newRuleReplacement) return;

    try {
      const newRule = await addCorrectionRule(
        newRulePattern,
        newRuleReplacement,
        newRuleIsRegex,
        0
      );
      setRules([...rules, newRule]);
      setNewRulePattern('');
      setNewRuleReplacement('');
      setNewRuleIsRegex(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to add rule');
    }
  }

  async function handleTrainCorrectionModel() {
    setTraining('correction');
    setError(null);
    try {
      const result = await trainCorrectionModel();
      if (!result.success) {
        setError(result.message);
      } else {
        await fetchData(); // Refresh stats
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Training failed');
    } finally {
      setTraining('none');
    }
  }

  async function handleTrainWhisperModel() {
    setTraining('whisper');
    setError(null);
    try {
      const result = await trainWhisperModel();
      if (!result.success) {
        setError(result.message);
      } else {
        await fetchData(); // Refresh stats
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Training failed');
    } finally {
      setTraining('none');
    }
  }

  if (loading) {
    return (
      <div className="learning-dashboard">
        <div className="dashboard-header">
          <h2>Learning Dashboard</h2>
          <button className="close-btn" onClick={onClose}>&times;</button>
        </div>
        <div className="dashboard-loading">Loading learning data...</div>
      </div>
    );
  }

  return (
    <div className="learning-dashboard">
      <div className="dashboard-header">
        <h2>Learning Dashboard</h2>
        <button className="close-btn" onClick={onClose}>&times;</button>
      </div>

      {error && <div className="dashboard-error">{error}</div>}

      <div className="dashboard-tabs">
        <button
          className={activeTab === 'overview' ? 'active' : ''}
          onClick={() => setActiveTab('overview')}
        >
          Overview
        </button>
        <button
          className={activeTab === 'corrections' ? 'active' : ''}
          onClick={() => setActiveTab('corrections')}
        >
          Corrections ({stats?.total_corrections || 0})
        </button>
        <button
          className={activeTab === 'rules' ? 'active' : ''}
          onClick={() => setActiveTab('rules')}
        >
          Rules ({rules.length})
        </button>
      </div>

      <div className="dashboard-content">
        {activeTab === 'overview' && stats && (
          <div className="overview-tab">
            <div className="stats-grid">
              <div className="stat-card">
                <div className="stat-value">{stats.total_corrections}</div>
                <div className="stat-label">Corrections Learned</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.total_applications}</div>
                <div className="stat-label">Times Applied</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">{stats.audio_samples}</div>
                <div className="stat-label">Audio Samples</div>
              </div>
              <div className="stat-card">
                <div className="stat-value">
                  {Math.round(stats.audio_duration_seconds / 60)}m
                </div>
                <div className="stat-label">Audio Duration</div>
              </div>
            </div>

            <div className="model-status">
              <h3>Model Status</h3>
              <div className="model-row">
                <span>Correction Model:</span>
                <span className={stats.correction_model_version ? 'trained' : 'not-trained'}>
                  {stats.correction_model_version || 'Not trained'}
                </span>
              </div>
              <div className="model-row">
                <span>Whisper Model:</span>
                <span className={stats.whisper_model_version ? 'trained' : 'not-trained'}>
                  {stats.whisper_model_version || 'Not trained'}
                </span>
              </div>
            </div>

            {Object.keys(stats.corrections_by_type).length > 0 && (
              <div className="corrections-by-type">
                <h3>Corrections by Type</h3>
                <div className="type-list">
                  {Object.entries(stats.corrections_by_type).map(([type, count]) => (
                    <div key={type} className="type-item">
                      <span className="type-name">{type}</span>
                      <span className="type-count">{count}</span>
                    </div>
                  ))}
                </div>
              </div>
            )}

            <div className="training-section">
              <h3>Training</h3>
              <div style={{ display: 'flex', gap: '12px', flexWrap: 'wrap' }}>
                <div>
                  <button
                    className="train-btn"
                    onClick={handleTrainCorrectionModel}
                    disabled={training !== 'none' || stats.total_corrections < 50}
                  >
                    {training === 'correction' ? 'Training...' : 'Train Correction Model'}
                  </button>
                  {stats.total_corrections < 50 && (
                    <p className="training-hint">
                      Need {50 - stats.total_corrections} more corrections
                    </p>
                  )}
                </div>
                <div>
                  <button
                    className="train-btn"
                    onClick={handleTrainWhisperModel}
                    disabled={training !== 'none' || !stats.ready_for_whisper_training}
                    style={{ background: stats.ready_for_whisper_training ? '#22c55e' : undefined }}
                  >
                    {training === 'whisper' ? 'Training...' : 'Train Whisper Model'}
                  </button>
                  {!stats.ready_for_whisper_training && (
                    <p className="training-hint">
                      Need {50 - stats.audio_samples} more audio samples
                    </p>
                  )}
                </div>
              </div>
            </div>
          </div>
        )}

        {activeTab === 'corrections' && (
          <div className="corrections-tab">
            <div className="corrections-list">
              {corrections.length === 0 ? (
                <p className="empty-state">
                  No corrections yet. Edit transcriptions to teach the system!
                </p>
              ) : (
                corrections.map(correction => (
                  <div key={correction.id} className="correction-item">
                    <div className="correction-texts">
                      <div className="original">
                        <span className="label">Original:</span>
                        {correction.original_text}
                      </div>
                      <div className="corrected">
                        <span className="label">Corrected:</span>
                        {correction.corrected_text}
                      </div>
                    </div>
                    <div className="correction-meta">
                      {correction.correction_type && (
                        <span className="type">{correction.correction_type}</span>
                      )}
                      <span className="count">Used {correction.correction_count}x</span>
                      <button
                        className="delete-btn"
                        onClick={() => handleDeleteCorrection(correction.id)}
                        title="Delete this correction"
                      >
                        &times;
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        )}

        {activeTab === 'rules' && (
          <div className="rules-tab">
            <div className="add-rule-form">
              <h3>Add Custom Rule</h3>
              <div className="form-row">
                <input
                  type="text"
                  placeholder="Pattern (text to find)"
                  value={newRulePattern}
                  onChange={e => setNewRulePattern(e.target.value)}
                />
                <input
                  type="text"
                  placeholder="Replacement"
                  value={newRuleReplacement}
                  onChange={e => setNewRuleReplacement(e.target.value)}
                />
              </div>
              <div className="form-row">
                <label>
                  <input
                    type="checkbox"
                    checked={newRuleIsRegex}
                    onChange={e => setNewRuleIsRegex(e.target.checked)}
                  />
                  Regular expression
                </label>
                <button onClick={handleAddRule} disabled={!newRulePattern || !newRuleReplacement}>
                  Add Rule
                </button>
              </div>
            </div>

            <div className="rules-list">
              {rules.length === 0 ? (
                <p className="empty-state">
                  No custom rules yet. Add rules above for instant corrections!
                </p>
              ) : (
                rules.map(rule => (
                  <div key={rule.id} className="rule-item">
                    <div className="rule-content">
                      <span className="pattern">
                        {rule.is_regex ? '/' : '"'}{rule.pattern}{rule.is_regex ? '/' : '"'}
                      </span>
                      <span className="arrow">&rarr;</span>
                      <span className="replacement">"{rule.replacement}"</span>
                    </div>
                    <div className="rule-meta">
                      <span className="hit-count">Used {rule.hit_count}x</span>
                      <button
                        className="delete-btn"
                        onClick={() => handleDeleteRule(rule.id)}
                        title="Delete this rule"
                      >
                        &times;
                      </button>
                    </div>
                  </div>
                ))
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
