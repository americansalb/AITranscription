/**
 * Reusable empty state component — ensures no page is ever a blank screen.
 * Every empty view gets an icon, title, description, and optional CTA.
 */

interface EmptyStateProps {
  icon: string;
  title: string;
  description: string;
  actionLabel?: string;
  onAction?: () => void;
}

export function EmptyState({ icon, title, description, actionLabel, onAction }: EmptyStateProps) {
  return (
    <div className="empty-state" role="status">
      <div className="empty-state-icon" aria-hidden="true">{icon}</div>
      <div className="empty-state-title">{title}</div>
      <div className="empty-state-desc">{description}</div>
      {actionLabel && onAction && (
        <button className="btn btn-primary" onClick={onAction}>
          {actionLabel}
        </button>
      )}
    </div>
  );
}
