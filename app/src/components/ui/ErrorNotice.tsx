import { messages } from "../../messages";
import { guidanceFromError } from "../../guidance";
import type { ErrorGuidance, GuidanceActionKind } from "../../guidance";

interface ErrorNoticeProps {
  error?: unknown;
  guidance?: ErrorGuidance;
  onAction?: (kind: GuidanceActionKind) => void;
}

const ACTION_LABELS: Record<GuidanceActionKind, string> = {
  retry: messages.error.actionRetry,
  "connect-ai": messages.error.actionConnectAi,
  "open-with-app": messages.error.actionOpenWithApp,
  dismiss: messages.common.close,
};

export default function ErrorNotice({ error, guidance, onAction }: ErrorNoticeProps) {
  const resolved = guidance ?? (error !== undefined ? guidanceFromError(error) : undefined);
  if (!resolved) return null;

  return (
    <div className="error-notice" role="alert">
      <p className="error-notice-title">{resolved.title}</p>
      <p className="error-notice-body">{resolved.message}</p>
      {resolved.actions.length > 0 && (
        <div className="error-notice-actions">
          {resolved.actions.map((kind) => (
            <button
              key={kind}
              type="button"
              className={kind === "connect-ai" ? "primary" : "secondary"}
              onClick={() => onAction?.(kind)}
            >
              {ACTION_LABELS[kind]}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
