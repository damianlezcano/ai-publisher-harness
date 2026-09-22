import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { TurnMetrics } from "../types";
import { formatElapsed, messages, turnTimestamp } from "../messages";

const UNAVAILABLE = messages.conversationDetails.metrics.unavailable;

/**
 * Compact, keyboard-accessible per-turn metrics line for a completed assistant
 * response. The detailed metrics + provenance live in a viewport-clamped
 * popover rendered through a portal into `document.body`, reusing the same
 * pattern as the import-progress details affordance. Only provider-actual
 * token values are shown; local Knowledge estimates stay clearly separated and
 * never substitute for provider telemetry.
 */
export default function AssistantMetrics({
  messageId,
  createdAt,
  turnMetrics,
}: {
  messageId: string;
  createdAt: string;
  turnMetrics: TurnMetrics;
}) {
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const suppressFocusOpen = useRef(false);
  const detailsId = `turn-metrics-${messageId}`;

  const position = useCallback(() => {
    const button = buttonRef.current;
    const panel = panelRef.current;
    if (!button || !panel) return;
    const rect = button.getBoundingClientRect();
    const gap = 8;
    const margin = 8;
    const panelWidth = panel.offsetWidth;
    const panelHeight = panel.offsetHeight;
    const viewportWidth = window.innerWidth;
    const viewportHeight = window.innerHeight;

    let top = rect.bottom + gap;
    if (top + panelHeight > viewportHeight - margin) {
      top = rect.top - gap - panelHeight;
      if (top < margin) top = margin;
    }

    let left = rect.right - panelWidth;
    if (left < margin) left = margin;
    if (left + panelWidth > viewportWidth - margin) {
      left = Math.max(margin, viewportWidth - margin - panelWidth);
    }

    panel.style.top = `${top}px`;
    panel.style.left = `${left}px`;
    panel.style.visibility = "visible";
  }, []);

  useLayoutEffect(() => {
    if (open) position();
  }, [open, position]);

  useEffect(() => {
    if (!open) return;
    function onViewportChange() {
      position();
    }
    window.addEventListener("resize", onViewportChange);
    window.addEventListener("scroll", onViewportChange, true);
    return () => {
      window.removeEventListener("resize", onViewportChange);
      window.removeEventListener("scroll", onViewportChange, true);
    };
  }, [open, position]);

  useEffect(() => {
    if (!open) return;
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }
    function handlePointerDown(event: MouseEvent) {
      const target = event.target as Node;
      if (buttonRef.current?.contains(target)) return;
      if (panelRef.current?.contains(target)) return;
      setOpen(false);
    }
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handlePointerDown);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handlePointerDown);
    };
  }, [open]);

  const segments = compactSegments(turnMetrics, createdAt);

  return (
    <span className="turn-metrics-line">
      {segments.length > 0 && <span className="turn-metrics-segments">{segments.join(" · ")}</span>}
      <button
        ref={buttonRef}
        type="button"
        className="turn-metrics-info-button"
        aria-label={messages.turnMetrics.infoAria}
        aria-expanded={open}
        aria-controls={detailsId}
        onMouseDown={() => {
          suppressFocusOpen.current = true;
          setTimeout(() => {
            suppressFocusOpen.current = false;
          }, 0);
        }}
        onFocus={() => {
          if (suppressFocusOpen.current) {
            suppressFocusOpen.current = false;
            return;
          }
          setOpen(true);
        }}
        onClick={() => setOpen((previous) => !previous)}
      >
        <span aria-hidden="true">ⓘ</span>
      </button>
      {open &&
        createPortal(
          <div
            ref={panelRef}
            id={detailsId}
            role="region"
            aria-label={messages.turnMetrics.detailsLabel}
            aria-live="off"
            className="turn-metrics-details"
            style={{ position: "fixed", top: 0, left: 0, visibility: "hidden" }}
          >
            <TurnDetails createdAt={createdAt} turnMetrics={turnMetrics} />
          </div>,
          document.body,
        )}
    </span>
  );
}

function compactSegments(metrics: TurnMetrics, createdAt: string): string[] {
  const parts: string[] = [turnTimestamp(createdAt)];
  if (metrics.turnDurationMs != null) parts.push(formatDuration(metrics.turnDurationMs));
  if (metrics.source === "provider_actual") {
    if (metrics.inputTokens != null) parts.push(`↑ ${formatTokens(metrics.inputTokens)}`);
    if (metrics.outputTokens != null) parts.push(`↓ ${formatTokens(metrics.outputTokens)}`);
  }
  return parts;
}

function formatTokens(value: number): string {
  return value.toLocaleString("es-AR");
}

function formatDuration(ms: number): string {
  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${seconds.toLocaleString("es-AR", { maximumFractionDigits: 1 })} s`;
  }
  return formatElapsed(ms);
}

function Detail({ label, value }: { label: string; value: string }) {
  return (
    <div className="turn-metrics-detail">
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    </div>
  );
}

function unavailable(value: number | null | undefined, suffix = ""): string {
  return value == null ? UNAVAILABLE : `${value.toLocaleString("es-AR")}${suffix}`;
}

function TurnDetails({
  createdAt,
  turnMetrics: m,
}: {
  createdAt: string;
  turnMetrics: TurnMetrics;
}) {
  const cdm = messages.conversationDetails.metrics;
  const actual = m.source === "provider_actual";
  const cost =
    actual && m.costUsd != null
      ? `USD ${m.costUsd.toLocaleString("en-US", { maximumFractionDigits: 6 })}`
      : UNAVAILABLE;
  const knowledgeUsed = isKnowledgeUsed(m);
  const knowledgeLabel = knowledgeUsed
    ? messages.turnMetrics.knowledgeUsed
    : messages.turnMetrics.knowledgeNotUsed;

  return (
    <>
      <section
        className="turn-metrics-subsection"
        aria-label={messages.turnMetrics.responseHeading}
      >
        <h4>{messages.turnMetrics.responseHeading}</h4>
        <dl className="turn-metrics-grid">
          <Detail label={messages.turnMetrics.dateTime} value={turnTimestamp(createdAt)} />
          <Detail label={cdm.turnDuration} value={unavailable(m.turnDurationMs, " ms")} />
          <Detail label={cdm.provider} value={m.provider || UNAVAILABLE} />
          <Detail label={cdm.model} value={m.model || UNAVAILABLE} />
        </dl>
      </section>
      <section className="turn-metrics-subsection" aria-label={cdm.realProvider}>
        <h4>{cdm.realProvider}</h4>
        <dl className="turn-metrics-grid">
          <Detail
            label={cdm.inputTokens}
            value={actual ? unavailable(m.inputTokens, " tokens") : UNAVAILABLE}
          />
          <Detail
            label={cdm.outputTokens}
            value={actual ? unavailable(m.outputTokens, " tokens") : UNAVAILABLE}
          />
          <Detail
            label={cdm.cacheReadTokens}
            value={actual ? unavailable(m.cacheReadTokens, " tokens") : UNAVAILABLE}
          />
          <Detail
            label={cdm.cacheWriteTokens}
            value={actual ? unavailable(m.cacheWriteTokens, " tokens") : UNAVAILABLE}
          />
          <Detail label={cdm.actualCost} value={cost} />
          <Detail label={cdm.remoteCalls} value={unavailable(m.remoteCalls ?? null)} />
        </dl>
      </section>
      <section
        className="turn-metrics-subsection"
        aria-label={messages.turnMetrics.knowledgeHeading}
      >
        <h4>{messages.turnMetrics.knowledgeHeading}</h4>
        <dl className="turn-metrics-grid">
          <Detail label={messages.turnMetrics.knowledgeHeading} value={knowledgeLabel} />
        </dl>
      </section>
    </>
  );
}

function isKnowledgeUsed(m: TurnMetrics): boolean {
  if (m.retrievalMode != null) return true;
  if (m.localMode != null) return true;
  return false;
}
