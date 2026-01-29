import { useEffect, useState, useRef } from 'react';
import { Insight } from '../../lib/insightGenerator';

interface InsightCardProps {
  insight: Insight;
  index?: number;
  animated?: boolean;
}

/**
 * Spotify Wrapped-style insight card with gradient background
 */
export function InsightCard({ insight, index = 0, animated = true }: InsightCardProps) {
  const [isVisible, setIsVisible] = useState(!animated);
  const cardRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!animated) return;

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          // Stagger animation based on index
          setTimeout(() => {
            setIsVisible(true);
          }, index * 100);
          observer.disconnect();
        }
      },
      { threshold: 0.2 }
    );

    if (cardRef.current) {
      observer.observe(cardRef.current);
    }

    return () => observer.disconnect();
  }, [animated, index]);

  return (
    <div
      ref={cardRef}
      className={`insight-card ${isVisible ? 'visible' : ''}`}
      style={{
        '--card-gradient': insight.gradient,
      } as React.CSSProperties}
    >
      <div className="insight-card-content">
        <div className="insight-icon">{insight.icon}</div>
        <div className="insight-title">{insight.title}</div>
        <div className="insight-subtitle">{insight.subtitle}</div>
        <div className="insight-description">{insight.description}</div>
      </div>
      <div className="insight-card-glow" />
    </div>
  );
}

interface InsightsTabProps {
  insights: Insight[];
}

/**
 * Full insights tab with scrollable narrative cards
 */
export function InsightsTab({ insights }: InsightsTabProps) {
  if (insights.length === 0) {
    return (
      <div className="insights-empty">
        <div className="empty-icon">📊</div>
        <h3>Your Story is Being Written</h3>
        <p>Keep transcribing to unlock personalized insights about your productivity patterns!</p>
      </div>
    );
  }

  return (
    <div className="insights-tab">
      <div className="insights-header">
        <h2>Your Story</h2>
        <p className="insights-subtitle">Discover what your data says about you</p>
      </div>
      <div className="insights-cards">
        {insights.map((insight, index) => (
          <InsightCard
            key={insight.id}
            insight={insight}
            index={index}
            animated={true}
          />
        ))}
      </div>
    </div>
  );
}

interface HighlightInsightProps {
  insight: Insight;
}

/**
 * Single highlight insight for overview display
 */
export function HighlightInsight({ insight }: HighlightInsightProps) {
  return (
    <div
      className="highlight-insight"
      style={{
        '--card-gradient': insight.gradient,
      } as React.CSSProperties}
    >
      <span className="highlight-icon">{insight.icon}</span>
      <div className="highlight-content">
        <span className="highlight-title">{insight.title}</span>
        <span className="highlight-value">{insight.subtitle}</span>
      </div>
    </div>
  );
}
