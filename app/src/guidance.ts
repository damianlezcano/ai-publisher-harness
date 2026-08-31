import { messages } from "./messages";

export type GuidanceActionKind = "retry" | "connect-ai" | "open-with-app" | "dismiss";

export interface ErrorGuidance {
  title: string;
  message: string;
  actions: GuidanceActionKind[];
}

interface ErrorCopy {
  title: string;
  message: string;
  hint?: string;
}

function compose(copy: ErrorCopy, actions: GuidanceActionKind[]): ErrorGuidance {
  return {
    title: copy.title,
    message: copy.hint ? `${copy.title} ${copy.hint}` : copy.message,
    actions,
  };
}

const GUIDANCE: Record<string, ErrorGuidance> = {
  ai_unavailable: compose(messages.error.aiUnavailable, ["retry"]),
  ai_task_failed: compose(messages.error.aiTaskFailed, ["retry"]),
  publish_failed: compose(messages.error.publishFailed, ["retry"]),
  network_error: compose(messages.error.networkError, ["retry"]),
  material_failed: compose(messages.error.materialFailed, []),
  material_unsupported: compose(messages.error.materialUnsupported, []),
  material_duplicate: compose(messages.error.materialDuplicate, []),
  preview_unavailable: compose(messages.error.previewUnavailable, ["open-with-app"]),
  preview_too_large: compose(messages.error.previewTooLarge, ["open-with-app"]),
  credential_revoked: compose(messages.error.credentialRevoked, ["connect-ai"]),
  credential_invalid: compose(messages.error.credentialInvalid, ["connect-ai"]),
  provider_unavailable: compose(messages.error.providerUnavailable, ["connect-ai"]),
  no_compatible_model: compose(messages.error.noCompatibleModel, ["connect-ai"]),
  model_unavailable: compose(messages.error.modelUnavailable, ["connect-ai"]),
  open_failed: compose(messages.error.openFailed, ["retry"]),
  storage_unavailable: compose(messages.error.storageUnavailable, ["retry"]),
  internal: compose(messages.error.internal, ["retry"]),
};

const FALLBACK: ErrorGuidance = compose(messages.error.internal, ["retry"]);

export function errorGuidance(code: string): ErrorGuidance {
  return GUIDANCE[code] ?? FALLBACK;
}

export function guidanceFromError(error: unknown): ErrorGuidance {
  if (typeof error === "object" && error !== null && "code" in error) {
    const code = (error as { code?: unknown }).code;
    if (typeof code === "string") {
      return errorGuidance(code);
    }
  }
  return FALLBACK;
}
