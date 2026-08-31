interface EmptyStateProps {
  title: string;
  body?: string;
  actionLabel?: string;
  onAction?: () => void;
  secondaryLabel?: string;
  onSecondary?: () => void;
}

export default function EmptyState({
  title,
  body,
  actionLabel,
  onAction,
  secondaryLabel,
  onSecondary,
}: EmptyStateProps) {
  return (
    <div className="empty-state">
      <p className="empty-state-title">{title}</p>
      {body && <p className="empty-state-body">{body}</p>}
      {(actionLabel || secondaryLabel) && (
        <div className="empty-state-actions">
          {actionLabel && (
            <button type="button" className="primary" onClick={onAction}>
              {actionLabel}
            </button>
          )}
          {secondaryLabel && (
            <button type="button" className="secondary" onClick={onSecondary}>
              {secondaryLabel}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
