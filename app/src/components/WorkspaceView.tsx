import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isAppError, type StagedImagePayload } from "../api";
import { guidanceFromError } from "../guidance";
import type { GuidanceActionKind } from "../guidance";
import type {
  AcceptedImportProgressView,
  AgentPhase,
  BackendReadiness,
  MaterialImportResult,
  ProjectView,
  StagedAttachmentView,
} from "../types";
import ChatPanel from "./ChatPanel";
import ComposerBar from "./ComposerBar";
import ShareControl from "./PublishPanel";
import { useShareControl } from "./useShareControl";
import ErrorNotice from "./ui/ErrorNotice";
import { formatElapsed, messages } from "../messages";
import ConversationDetails from "./ConversationDetails";
import { isNearBottom, scrollToLatest } from "../scroll";
import { embeddingProgressPercent } from "../importProgress";

interface WorkspaceViewProps {
  project: ProjectView;
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onBack: () => void;
  onRefresh: () => void | Promise<void>;
  onSendStart?: () => void;
  onSendEnd?: (projectId: string) => void;
  aiUsable: boolean;
  backendStatus?: BackendReadiness;
  onRetryBackend?: () => void;
  onOpenProvider: () => void;
  onProviderError: () => void;
  resumeFailure?: string | null;
  resumeNoTurn?: string | null;
  onResumeRetry?: (operationId: string) => void;
  onRetrySummary?: (operationId: string) => void;
  draft?: string;
  onDraftChange?: (value: string) => void;
}

function importDetailLabel(item: MaterialImportResult): string {
  switch (item.status) {
    case "added":
      return messages.material.perFileAdded(item.sourceName);
    case "duplicate":
      return messages.material.perFileDuplicate(item.sourceName);
    case "duplicate_in_batch":
      return messages.material.perFileDuplicateInBatch(item.sourceName);
    default:
      return messages.material.perFileFailed(item.sourceName);
  }
}

/**
 * Compact, keyboard-accessible details affordance for the accepted-import
 * progress line. The detailed panel is rendered through a React portal into
 * `document.body` so the scrolling timeline/chat container can never clip it.
 * It is anchored to the ⓘ button's bounding box and flipped/clamped to stay
 * inside the viewport. It is a real `<button>`: a pointer click toggles the
 * panel open/closed and keyboard focus reveals it, while `Escape` or an
 * outside click closes it. The panel is `aria-live="off"` so the frequently
 * changing per-poll values never produce a stream of screen-reader
 * announcements; only the compact status line (the `role="status"` section) is
 * announced.
 */
function ImportProgressInfo({
  accepted,
  showSpeed,
}: {
  accepted: AcceptedImportProgressView;
  showSpeed: boolean;
}) {
  const [open, setOpen] = useState(false);
  const buttonRef = useRef<HTMLButtonElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const suppressFocusOpen = useRef(false);
  const detailsId = "import-progress-details";

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

    // Prefer below the button; flip above when it would overflow the viewport.
    let top = rect.bottom + gap;
    if (top + panelHeight > viewportHeight - margin) {
      top = rect.top - gap - panelHeight;
      if (top < margin) top = margin;
    }

    // Prefer right-aligned to the button; clamp inside horizontal margins.
    let left = rect.right - panelWidth;
    if (left < margin) left = margin;
    if (left + panelWidth > viewportWidth - margin) {
      left = Math.max(margin, viewportWidth - margin - panelWidth);
    }

    // Position imperatively on the DOM node (measure-then-move), never via
    // React state, so the effect stays a side-effect-free DOM synchronization.
    panel.style.top = `${top}px`;
    panel.style.left = `${left}px`;
    panel.style.visibility = "visible";
  }, []);

  useLayoutEffect(() => {
    if (!open) return;
    position();
  }, [open, position]);

  // Re-measure when the detail content changes size (progress updates) while
  // open, so the panel stays clamped to the viewport.
  useLayoutEffect(() => {
    if (open) position();
  }, [accepted, open, position]);

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

  const ready = accepted.materialsReady;
  const total = accepted.total;
  const embeddingKnown = accepted.embeddingsTotal > 0;
  const rate = accepted.throughputEmbeddingsPerSec;

  return (
    <span className="import-progress-info">
      <button
        ref={buttonRef}
        type="button"
        className="import-progress-info-button"
        aria-label={messages.progressDetails.infoAria}
        aria-expanded={open}
        aria-controls={detailsId}
        onMouseDown={() => {
          // A pointer click focuses the button before the click event; the
          // focus-open must not fire first or the toggle would immediately
          // close what focus just opened.
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
        onBlur={() => setOpen(false)}
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
            aria-label={messages.progressDetails.infoAria}
            aria-live="off"
            className="import-progress-details"
            style={{
              position: "fixed",
              top: 0,
              left: 0,
              visibility: "hidden",
            }}
          >
            <dl className="import-progress-detail-grid">
              <div className="import-progress-detail">
                <dt>{messages.progressDetails.readyLabel}</dt>
                <dd>{`${ready} de ${total}`}</dd>
              </div>
              <div className="import-progress-detail">
                <dt>{messages.progressDetails.errorsLabel}</dt>
                <dd>{accepted.failed}</dd>
              </div>
              <div className="import-progress-detail">
                <dt>{messages.progressDetails.preparedLabel}</dt>
                <dd>{`${accepted.copied} / ${total}`}</dd>
              </div>
              {accepted.chunksTotal > 0 && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.fragmentsLabel}</dt>
                  <dd>{accepted.chunksTotal.toLocaleString("es-AR")}</dd>
                </div>
              )}
              {embeddingKnown && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.embeddingsLabel}</dt>
                  <dd>{`${accepted.embeddingCompleted.toLocaleString("es-AR")} / ${accepted.embeddingsTotal.toLocaleString("es-AR")}`}</dd>
                </div>
              )}
              {embeddingKnown && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.embeddingsCreatedLabel}</dt>
                  <dd>{accepted.embeddingsCreated.toLocaleString("es-AR")}</dd>
                </div>
              )}
              {accepted.embeddingsReused > 0 && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.embeddingsReusedLabel}</dt>
                  <dd>{accepted.embeddingsReused.toLocaleString("es-AR")}</dd>
                </div>
              )}
              {accepted.elapsedMs > 0 && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.elapsedLabel}</dt>
                  <dd>{formatElapsed(accepted.elapsedMs)}</dd>
                </div>
              )}
              {showSpeed && rate != null && (
                <div className="import-progress-detail">
                  <dt>{messages.progressDetails.speedLabel}</dt>
                  <dd>{`~${Math.round(rate).toLocaleString("es-AR")} /s`}</dd>
                </div>
              )}
            </dl>
          </div>,
          document.body,
        )}
    </span>
  );
}

export default function WorkspaceView(props: WorkspaceViewProps) {
  const {
    project,
    agentPhase,
    agentMessage,
    onRefresh,
    onSendStart,
    onSendEnd,
    aiUsable,
    backendStatus = "starting",
    onRetryBackend,
    onOpenProvider,
    onProviderError,
    resumeFailure,
    resumeNoTurn,
    onResumeRetry,
    onRetrySummary,
    draft,
    onDraftChange,
  } = props;

  const [sendError, setSendError] = useState<unknown | null>(null);
  const [refreshError, setRefreshError] = useState<unknown | null>(null);
  const [lastAttempt, setLastAttempt] = useState<{
    text: string;
    materialIds: string[];
    stagedImages: StagedImagePayload[];
  } | null>(null);
  const [materialError, setMaterialError] = useState<unknown | null>(null);
  const [importNotice, setImportNotice] = useState<string | null>(null);
  const [importDetails, setImportDetails] = useState<MaterialImportResult[] | null>(null);
  const [importDetailsOpen, setImportDetailsOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [importing, setImporting] = useState(false);
  const [attachmentIds, setAttachmentIds] = useState<string[]>([]);
  const [stagedAttachments, setStagedAttachments] = useState<Map<string, StagedAttachmentView>>(
    new Map(),
  );
  const [detailsOpen, setDetailsOpen] = useState(false);

  const timelineRef = useRef<HTMLDivElement>(null);
  const nearBottomRef = useRef(true);

  useLayoutEffect(() => {
    const el = timelineRef.current;
    if (!el) return;
    const frame = requestAnimationFrame(() => scrollToLatest(el));
    return () => cancelAnimationFrame(frame);
  }, [project.id]);

  useEffect(() => {
    if (!nearBottomRef.current) return;
    const el = timelineRef.current;
    if (!el) return;
    const frame = requestAnimationFrame(() => scrollToLatest(el));
    return () => cancelAnimationFrame(frame);
  }, [project.messages, agentPhase]);

  function handleTimelineScroll() {
    const el = timelineRef.current;
    if (!el) return;
    nearBottomRef.current = isNearBottom(el);
  }

  useEffect(() => {
    if (agentPhase !== "working") return;
    const timer = window.setInterval(() => void onRefresh(), 500);
    return () => window.clearInterval(timer);
  }, [agentPhase, onRefresh]);

  const importRef = useRef<(paths: string[]) => Promise<void>>(async () => {});

  const workspaceClass = useMemo(
    () => `view workspace workspace-chat${dragging ? " is-dropping" : ""}`,
    [dragging],
  );

  const share = useShareControl({
    projectId: project.id,
    publication: project.publication,
    onRefresh,
  });

  useEffect(() => {
    importRef.current = async (paths: string[]) => {
      if (paths.length === 0) return;
      setImporting(true);
      setMaterialError(null);
      setImportNotice(null);
      setImportDetails(null);
      setImportDetailsOpen(false);
      try {
        const report = await api.attachmentsStagePaths(paths);
        const acceptedPaths = paths.filter((_, index) => {
          const status = report.items[index]?.status;
          return status === "ready" || status === "duplicate_in_selection";
        });
        setAttachmentIds((previous) => [...new Set([...previous, ...acceptedPaths])]);
        setStagedAttachments((previous) => {
          const next = new Map(previous);
          paths.forEach((path, index) => {
            const item = report.items[index];
            if (item) next.set(path, item);
          });
          return next;
        });
        const failed = report.items.filter(
          (item) => item.status === "unsupported" || item.status === "failed",
        ).length;
        if (failed > 0) setMaterialError({ code: "attachment_invalid", message: "" });
      } catch (err) {
        setMaterialError(err);
      } finally {
        setImporting(false);
      }
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (!active) return;
        if (event.payload.type === "over") {
          setDragging(true);
        } else if (event.payload.type === "drop") {
          setDragging(false);
          void importRef.current(event.payload.paths);
        } else if (event.payload.type === "leave") {
          setDragging(false);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const sendingRef = useRef(false);

  useEffect(() => {
    if (agentPhase !== "working") {
      sendingRef.current = false;
    }
  }, [agentPhase]);

  async function send(
    text: string,
    attachmentIds: string[],
    stagedImages: StagedImagePayload[] = [],
  ) {
    if (sendingRef.current || agentPhase === "working") return;
    sendingRef.current = true;
    onSendStart?.();
    setSendError(null);
    setRefreshError(null);
    setLastAttempt({ text, materialIds: attachmentIds, stagedImages });
    const stagedImageIds = new Set(stagedImages.map((image) => image.stagingId));
    try {
      await api.agentSendStaged(
        project.id,
        text,
        attachmentIds.filter((id) => !stagedImageIds.has(id)),
        stagedImages,
      );
    } catch (err) {
      // PRE-ACCEPT failure: the turn was never accepted, so ComposerBar may
      // restore the submitted draft and keep the pending selection for a retry.
      sendingRef.current = false;
      onSendEnd?.(project.id);
      const transient =
        isAppError(err) && err.code === "ai_unavailable" && backendStatus === "starting";
      if (!transient) {
        setSendError(err);
        if (guidanceFromError(err).actions.includes("connect-ai")) {
          onProviderError();
        }
      }
      // Reject so ComposerBar only clears its local pending state once the
      // accepted turn has actually been committed.
      throw err;
    }

    // ACCEPTANCE COMMIT POINT: `agentSendStaged` resolved, so the user turn is
    // durably accepted. From here the submitted draft must never come back, and
    // the pending selection is committed. The turn stays in flight, so the
    // composer remains disabled until the agent task reports its result.
    setStagedAttachments(new Map());
    onDraftChange?.("");
    try {
      await onRefresh();
    } catch (err) {
      // POST-ACCEPT refresh/UI-sync failure. The turn already exists, so do not
      // reject through ComposerBar's "send failed" path (that would restore the
      // draft and invite a duplicate manual send). Report it separately without
      // any resend action.
      console.error("[send] accepted turn; refresh failed after commit", err);
      setRefreshError(err);
    }
  }

  async function retrySend() {
    if (!lastAttempt) return;
    await send(lastAttempt.text, lastAttempt.materialIds, lastAttempt.stagedImages);
  }

  function handleSendErrorAction(kind: GuidanceActionKind) {
    if (kind === "retry") {
      void retrySend();
    } else if (kind === "connect-ai") {
      onProviderError();
    }
  }

  async function cancel() {
    setSendError(null);
    try {
      await api.agentCancel(project.id);
      await onRefresh();
    } catch (err) {
      setSendError(err);
    }
  }

  async function resumeImport(operationId: string) {
    if (onResumeRetry) {
      onResumeRetry(operationId);
      return;
    }
    await api.agentResumeImport(project.id, operationId);
    await onRefresh();
  }

  async function retrySummary(operationId: string) {
    if (onRetrySummary) {
      onRetrySummary(operationId);
      return;
    }
    await api.agentRetrySummary(project.id, operationId);
    await onRefresh();
  }

  // A zero-total durable operation is a normal no-attachment turn, not an
  // import. It must never suppress normal working copy or render 0/0 details.
  const acceptedImport = (project.acceptedImport?.total ?? 0) > 0 ? project.acceptedImport : null;
  // A no-attachment compact follow-up still runs a summary synthesis, but its
  // durable operation has total=0. Surface that synthesis phase in the chat
  // working line without an import-progress UI.
  const synthesizingSummaries =
    project.acceptedImport?.synthesizing === true && project.acceptedImport?.state !== "completed";
  const processingNotice = useMemo(() => {
    if (!acceptedImport) return null;
    if (acceptedImport.state === "completed") return null;
    if (acceptedImport.summaryRetryable) {
      return {
        text: messages.processing.outcomeUnknown,
        retry: true,
        retryKind: "summary" as const,
      };
    }
    if (acceptedImport.agentState === "started_outcome_unknown") {
      return {
        text: messages.processing.outcomeUnknown,
        retry: false,
        retryKind: "resume" as const,
      };
    }
    if (resumeNoTurn === project.id) {
      return { text: messages.processing.noTurn, retry: false };
    }
    if (
      acceptedImport.agentState === "failed_retryable" ||
      acceptedImport.agentState === "failed_terminal"
    ) {
      return { text: messages.processing.cannotContinue, retry: false };
    }
    if (resumeFailure === acceptedImport.operationId) {
      return { text: messages.processing.resumeFailed, retry: true, retryKind: "resume" as const };
    }
    if (acceptedImport.state === "pending_retry" && acceptedImport.agentState === "not_started") {
      return { text: messages.processing.pendingRetry, retry: true, retryKind: "resume" as const };
    }
    return null;
  }, [acceptedImport, resumeFailure, resumeNoTurn, project.id]);

  return (
    <div className={workspaceClass}>
      {dragging && (
        <div className="drop-overlay" role="status" aria-live="polite">
          {messages.material.dropOverlay}
        </div>
      )}

      <header className="view-header workspace-header">
        <h1>
          <button
            type="button"
            className="title-button"
            aria-label={messages.conversationDetails.titleAria(project.name)}
            onClick={() => setDetailsOpen(true)}
          >
            {project.name}
          </button>
        </h1>
      </header>

      <div className="workspace-timeline" ref={timelineRef} onScroll={handleTimelineScroll}>
        <ChatPanel
          projectId={project.id}
          messages={project.messages}
          materials={project.materials}
          creations={project.creations}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          onRefresh={onRefresh}
          suppressWorkingStatus={acceptedImport != null && acceptedImport.state !== "completed"}
          synthesizingSummaries={synthesizingSummaries}
          share={{
            onShare: (creationId: string) => {
              if (!share.shared) {
                void share.publish(creationId);
              }
            },
            shared: share.shared,
            busy: share.busy === "publishing",
          }}
        />
        {acceptedImport &&
          (() => {
            const accepted = acceptedImport;
            const completed = accepted.state === "completed";
            const ready = accepted.materialsReady;
            const total = accepted.total;
            const failed = accepted.failed;
            // `materialsReady` is the truthfully computed fully-usable count
            // for the active Knowledge generation (lexical + embeddings). The
            // compact line never claims N/N before every accepted material is
            // actually usable.
            const allReady = completed && ready >= total;
            const degradedTerminal = completed && !allReady;
            // The truthful synthesis phase replaces the stale "99% · N de N"
            // import line once embeddings complete and compact summary
            // generation starts. It is a single indeterminate phase, never a
            // fake per-file provider progress.
            const synthesizing = accepted.synthesizing === true && !completed;
            const embeddingActive = accepted.state === "indexing_embeddings" && !synthesizing;
            const embeddingKnown = accepted.embeddingsTotal > 0;
            const embeddingPercent = embeddingProgressPercent(accepted);
            const rate = accepted.throughputEmbeddingsPerSec;
            const showSpeed = embeddingActive && embeddingKnown && rate != null && rate >= 1;
            const countPart =
              failed > 0
                ? messages.compactProgress.readyCountWithErrors(ready, total, failed)
                : messages.compactProgress.readyCount(ready, total);
            const primary = synthesizing
              ? messages.compactProgress.generatingSummaries(total)
              : allReady
                ? messages.compactProgress.readyTotal(total)
                : completed
                  ? countPart
                  : [
                      messages.compactProgress.requestPrefix.replace(/\s*·\s*$/, ""),
                      embeddingPercent == null
                        ? null
                        : messages.compactProgress.embeddingPercent(embeddingPercent),
                      countPart,
                    ]
                      .filter((part): part is string => part != null)
                      .join(" · ");
            return (
              <section className="import-progress" role="status" aria-live="polite">
                <div className="import-progress-primary">
                  {!completed && <span className="spinner" aria-hidden="true" />}
                  <strong>{primary}</strong>
                  <ImportProgressInfo accepted={accepted} showSpeed={showSpeed} />
                </div>
                {degradedTerminal && (
                  <span className="import-progress-notice">
                    {messages.processing.semanticUnavailable}
                  </span>
                )}
                {processingNotice && (
                  <p className="import-progress-notice">{processingNotice.text}</p>
                )}
                {processingNotice?.retry && (
                  <button
                    type="button"
                    className="button-secondary"
                    onClick={() =>
                      void (processingNotice.retryKind === "summary"
                        ? retrySummary(accepted.operationId)
                        : resumeImport(accepted.operationId))
                    }
                  >
                    {messages.common.retry}
                  </button>
                )}
              </section>
            );
          })()}
        {importing && (
          <p className="notice import-result-status" role="status">
            <span className="spinner" aria-hidden="true" />
            {messages.progress.importing}
          </p>
        )}
        {importNotice && (
          <section className="import-result" aria-label="Resultado de archivos agregados">
            <p className="notice import-result-status">{importNotice}</p>
            {importDetails && importDetails.length > 0 && (
              <>
                <button
                  type="button"
                  className="button-secondary import-result-toggle"
                  aria-expanded={importDetailsOpen}
                  onClick={() => setImportDetailsOpen((open) => !open)}
                >
                  {importDetailsOpen ? "Ocultar detalle" : "Ver detalle"}
                </button>
                {importDetailsOpen && (
                  <ul className="chip-list import-result-details">
                    {importDetails.map((item, index) => (
                      <li key={`${item.sourceName}-${index}`} className="chip">
                        {importDetailLabel(item)}
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
          </section>
        )}
      </div>

      {materialError !== null && <ErrorNotice error={materialError} />}

      {backendStatus === "starting" && (
        <p className="notice composer-import-status" role="status">
          <span className="spinner" aria-hidden="true" />
          {messages.assistant.starting}
        </p>
      )}
      {sendError !== null && backendStatus !== "failed" && (
        <ErrorNotice error={sendError} onAction={handleSendErrorAction} />
      )}
      {refreshError !== null && (
        <ErrorNotice
          guidance={{
            title: messages.error.refreshAfterSend.title,
            message: messages.error.refreshAfterSend.message,
            actions: [],
          }}
        />
      )}
      {backendStatus === "failed" && (
        <ErrorNotice
          error={{ code: "ai_unavailable", message: messages.error.aiUnavailable.message }}
          onAction={(kind) => {
            if (kind === "retry") onRetryBackend?.();
          }}
        />
      )}

      <div className="workspace-composer">
        <ComposerBar
          materials={project.materials}
          agentPhase={agentPhase}
          aiUsable={aiUsable}
          onSend={send}
          onCancel={cancel}
          onOpenProvider={onOpenProvider}
          attachmentIds={attachmentIds}
          attachmentNames={stagedAttachments}
          prompt={draft}
          onPromptChange={onDraftChange}
          onAttachmentIdsChange={(ids) => {
            setAttachmentIds(ids);
            setStagedAttachments((previous) => {
              const next = new Map(previous);
              for (const path of previous.keys()) if (!ids.includes(path)) next.delete(path);
              return next;
            });
          }}
          shareAction={
            <ShareControl
              projectId={project.id}
              projectName={project.name}
              publication={project.publication}
              onRefresh={onRefresh}
              share={share}
            />
          }
        />
      </div>
      {detailsOpen && (
        <ConversationDetails
          key={project.id}
          project={project}
          active={agentPhase === "working"}
          onClose={() => setDetailsOpen(false)}
          onRefresh={onRefresh}
        />
      )}
    </div>
  );
}
