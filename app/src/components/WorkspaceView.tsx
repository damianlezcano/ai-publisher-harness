import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, isAppError, type StagedImagePayload } from "../api";
import { guidanceFromError } from "../guidance";
import type { GuidanceActionKind } from "../guidance";
import type {
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
import { messages } from "../messages";
import ConversationDetails from "./ConversationDetails";

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
  } = props;

  const [sendError, setSendError] = useState<unknown | null>(null);
  const [resumeError, setResumeError] = useState<unknown | null>(null);
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
    setLastAttempt({ text, materialIds: attachmentIds, stagedImages });
    try {
      const stagedImageIds = new Set(stagedImages.map((image) => image.stagingId));
      await api.agentSendStaged(
        project.id,
        text,
        attachmentIds.filter((id) => !stagedImageIds.has(id)),
        stagedImages,
      );
      await onRefresh();
      setStagedAttachments(new Map());
    } catch (err) {
      sendingRef.current = false;
      onSendEnd?.(project.id);
      const transient =
        isAppError(err) && err.code === "ai_unavailable" && backendStatus === "starting";
      if (transient) {
        // Defensive: the composer is gated while the backend is starting, so this
        // branch is only reachable in a narrow race. Keeping the optimistic bubble
        // avoids a false terminal error; if gating changes, restore the text to the
        // composer or auto-retry once the backend becomes ready.
      } else {
        setSendError(err);
        if (guidanceFromError(err).actions.includes("connect-ai")) {
          onProviderError();
        }
      }
      // ComposerBar owns the staged selection. Reject so it only clears that
      // local state once the accepted turn has actually been committed.
      throw err;
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
    setResumeError(null);
    try {
      await api.agentResumeImport(project.id, operationId);
      await onRefresh();
    } catch (err) {
      setResumeError(err);
    }
  }

  const acceptedImport = project.acceptedImport ?? null;
  const processingNotice = useMemo(() => {
    if (!acceptedImport) return null;
    if (acceptedImport.state === "completed") return null;
    if (acceptedImport.agentState === "started_outcome_unknown") {
      return { text: messages.processing.outcomeUnknown, retry: false };
    }
    if (
      acceptedImport.agentState === "failed_retryable" ||
      acceptedImport.agentState === "failed_terminal"
    ) {
      return { text: messages.processing.cannotContinue, retry: false };
    }
    if (acceptedImport.state === "pending_retry" && acceptedImport.agentState === "not_started") {
      return { text: messages.processing.pendingRetry, retry: true };
    }
    return null;
  }, [acceptedImport]);

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

      <div className="workspace-timeline">
        <ChatPanel
          projectId={project.id}
          messages={project.messages}
          materials={project.materials}
          creations={project.creations}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          onRefresh={onRefresh}
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
        {project.acceptedImport && (
          <section className="import-progress" role="status" aria-live="polite">
            <strong>{messages.processing.title(project.acceptedImport.total)}</strong>
            <span>
              {messages.processing.prepared(
                project.acceptedImport.copied,
                project.acceptedImport.total,
              )}
            </span>
            <span>
              {messages.processing.indexed(
                project.acceptedImport.lexicalCompleted,
                project.acceptedImport.total,
              )}
            </span>
            <span>
              {messages.processing.embeddings(
                project.acceptedImport.embeddingsCreated,
                project.acceptedImport.embeddingsReused,
              )}
            </span>
            <span>{messages.processing.ready(project.acceptedImport.embeddingCompleted)}</span>
            {project.acceptedImport.failed > 0 && (
              <span>{messages.processing.errors(project.acceptedImport.failed)}</span>
            )}
            {processingNotice && <p className="import-progress-notice">{processingNotice.text}</p>}
            {processingNotice?.retry && (
              <button
                type="button"
                className="button-secondary"
                onClick={() => void resumeImport(project.acceptedImport!.operationId)}
              >
                {messages.common.retry}
              </button>
            )}
          </section>
        )}
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
      {resumeError !== null && <ErrorNotice error={resumeError} />}

      {backendStatus === "starting" && (
        <p className="notice composer-import-status" role="status">
          <span className="spinner" aria-hidden="true" />
          {messages.assistant.starting}
        </p>
      )}
      {sendError !== null && backendStatus !== "failed" && (
        <ErrorNotice error={sendError} onAction={handleSendErrorAction} />
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
