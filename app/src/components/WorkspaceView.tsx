import { useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "../api";
import { guidanceFromError } from "../guidance";
import type { GuidanceActionKind } from "../guidance";
import type { AgentPhase, MaterialImportResult, ProjectView } from "../types";
import ChatPanel from "./ChatPanel";
import ComposerBar from "./ComposerBar";
import ShareControl from "./PublishPanel";
import ErrorNotice from "./ui/ErrorNotice";
import { messages } from "../messages";

interface WorkspaceViewProps {
  project: ProjectView;
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onBack: () => void;
  onRefresh: () => void | Promise<void>;
  aiUsable: boolean;
  onOpenProvider: () => void;
  onProviderError: () => void;
}

function importDetailLabel(item: MaterialImportResult): string {
  switch (item.status) {
    case "added":
      return messages.material.perFileAdded(item.sourceName);
    case "duplicate":
      return messages.material.perFileDuplicate(item.sourceName);
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
    aiUsable,
    onOpenProvider,
    onProviderError,
  } = props;

  const [pendingUser, setPendingUser] = useState<{ text: string; materialIds: string[] } | null>(
    null,
  );
  const [sendError, setSendError] = useState<unknown | null>(null);
  const [lastAttempt, setLastAttempt] = useState<{ text: string; materialIds: string[] } | null>(
    null,
  );
  const [materialError, setMaterialError] = useState<unknown | null>(null);
  const [importNotice, setImportNotice] = useState<string | null>(null);
  const [importDetails, setImportDetails] = useState<MaterialImportResult[] | null>(null);
  const [dragging, setDragging] = useState(false);
  const [importing, setImporting] = useState(false);

  const importRef = useRef<(paths: string[]) => Promise<void>>(async () => {});

  const workspaceClass = useMemo(
    () => `view workspace workspace-chat${dragging ? " is-dropping" : ""}`,
    [dragging],
  );

  useEffect(() => {
    importRef.current = async (paths: string[]) => {
      if (paths.length === 0) return;
      setImporting(true);
      setMaterialError(null);
      setImportNotice(null);
      setImportDetails(null);
      try {
        const report = await api.materialsAddFromPaths(project.id, paths);
        const added = report.items.filter((item) => item.status === "added").length;
        const duplicate = report.items.filter((item) => item.status === "duplicate").length;
        const failed = report.items.filter(
          (item) => item.status === "unsupported" || item.status === "failed",
        ).length;
        setImportNotice(messages.material.importSummary(added, duplicate, failed));
        if (duplicate > 0 || failed > 0) {
          setImportDetails(report.items);
        }
        await onRefresh();
      } catch (err) {
        setMaterialError(err);
      } finally {
        setImporting(false);
      }
    };
  }, [project.id, onRefresh]);

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

  async function send(text: string, attachmentIds: string[]) {
    setSendError(null);
    setLastAttempt({ text, materialIds: attachmentIds });
    setPendingUser({ text, materialIds: attachmentIds });
    try {
      await api.agentSend(project.id, text, attachmentIds);
      await onRefresh();
    } catch (err) {
      setPendingUser(null);
      setSendError(err);
      if (guidanceFromError(err).actions.includes("connect-ai")) {
        onProviderError();
      }
    }
  }

  async function retrySend() {
    if (!lastAttempt) return;
    await send(lastAttempt.text, lastAttempt.materialIds);
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

  return (
    <div className={workspaceClass}>
      {dragging && (
        <div className="drop-overlay" role="status" aria-live="polite">
          {messages.material.dropOverlay}
        </div>
      )}

      <header className="view-header workspace-header">
        <h1>{project.name}</h1>
      </header>

      <div className="workspace-timeline">
        <ChatPanel
          projectId={project.id}
          messages={project.messages}
          materials={project.materials}
          creations={project.creations}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          pendingUser={pendingUser}
          onRefresh={onRefresh}
        />
      </div>

      {materialError !== null && <ErrorNotice error={materialError} />}
      {importing && (
        <p className="notice composer-import-status" role="status">
          <span className="spinner" aria-hidden="true" />
          {messages.progress.importing}
        </p>
      )}
      {importNotice && <p className="notice composer-import-status">{importNotice}</p>}
      {importDetails && importDetails.length > 0 && (
        <ul className="chip-list composer-import-details">
          {importDetails.map((item) => (
            <li key={item.sourceName} className="chip">
              {importDetailLabel(item)}
            </li>
          ))}
        </ul>
      )}

      {sendError !== null && <ErrorNotice error={sendError} onAction={handleSendErrorAction} />}

      <div className="workspace-composer">
        <ComposerBar
          projectId={project.id}
          materials={project.materials}
          agentPhase={agentPhase}
          aiUsable={aiUsable}
          onSend={send}
          onCancel={cancel}
          onOpenProvider={onOpenProvider}
          onMaterialsChanged={() => void onRefresh()}
          shareAction={
            <ShareControl
              projectId={project.id}
              projectName={project.name}
              publication={project.publication}
              onRefresh={onRefresh}
            />
          }
        />
      </div>
    </div>
  );
}
