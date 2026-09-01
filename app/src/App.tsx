import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { AgentPhase, BackendReadiness, ProjectSummary, ProjectView } from "./types";
import { guidanceFromError } from "./guidance";
import WorkspaceView from "./components/WorkspaceView";
import ConversationsSidebar from "./components/ConversationsSidebar";
import ProviderPanel from "./components/provider/ProviderPanel";
import ProviderStatusBanner from "./components/ui/ProviderStatusBanner";
import type { ProviderStatus } from "./components/ui/ProviderStatusBanner";
import ToastRegion from "./components/ui/ToastRegion";
import { useToast } from "./components/ui/useToast";
import { conversationDisplayName, messages } from "./messages";

export default function App() {
  const [conversations, setConversations] = useState<ProjectSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [conversation, setConversation] = useState<ProjectView | null>(null);
  const [loading, setLoading] = useState(true);
  const [agentPhase, setAgentPhase] = useState<AgentPhase>("idle");
  const [agentMessage, setAgentMessage] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [providerRefreshKey, setProviderRefreshKey] = useState(0);
  const [providerStatus, setProviderStatus] = useState<ProviderStatus | null>(null);
  const [needsReconnect, setNeedsReconnect] = useState(false);
  const [backendStatus, setBackendStatus] = useState<BackendReadiness>("starting");
  const { toasts, show } = useToast();

  const refreshConversations = useCallback(async () => {
    const list = await api.projectList();
    setConversations(
      list.map((summary) => ({ ...summary, name: conversationDisplayName(summary.name) })),
    );
  }, []);

  const refreshConversation = useCallback(async (id: string) => {
    const view = await api.projectOpen(id);
    setConversation({ ...view, name: conversationDisplayName(view.name) });
  }, []);

  const openConversation = useCallback(
    async (id: string) => {
      setSelectedId(id);
      setAgentPhase("idle");
      setAgentMessage(null);
      try {
        const view = await api.projectOpen(id);
        setConversation({ ...view, name: conversationDisplayName(view.name) });
      } catch (error) {
        setConversation(null);
        show(guidanceFromError(error).title);
      }
    },
    [show],
  );

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const list = await api.projectList();
        if (!active) return;
        if (list.length === 0) {
          await api.projectCreate(messages.conversation.defaultName);
          if (!active) return;
          await refreshConversations();
          if (!active) return;
          const refreshed = await api.projectList();
          if (active && refreshed.length > 0) {
            await openConversation(refreshed[0].id);
          }
        } else {
          setConversations(
            list.map((summary) => ({ ...summary, name: conversationDisplayName(summary.name) })),
          );
          await openConversation(list[0].id);
        }
      } catch (error) {
        if (active) show(guidanceFromError(error).title);
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [openConversation, refreshConversations, show]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    void api
      .onAgentTask((event) => {
        if (!active || event.projectId !== selectedId) return;
        if (event.status === "working") {
          setAgentPhase("working");
          setAgentMessage(null);
        } else if (event.status === "completed") {
          setAgentPhase("completed");
          setAgentMessage(event.message ?? messages.agent.completed);
          show(messages.agent.ready);
        } else {
          setAgentPhase("failed");
          setAgentMessage(event.message ?? messages.agent.taskFailed);
        }
        if (selectedId) {
          void refreshConversation(selectedId);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [selectedId, refreshConversation, show]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const current = await api.modelGetSelected();
        if (!active) return;
        setProviderStatus(current.requiresChoice ? "requires-choice" : null);
      } catch {
        if (active) setProviderStatus(null);
      }
    })();
    return () => {
      active = false;
    };
  }, [providerRefreshKey]);

  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    async function check() {
      try {
        const status = await api.appStatus();
        if (!active) return;
        const next: BackendReadiness =
          status.agent === "ready" ? "ready" : status.agent === "failed" ? "failed" : "starting";
        setBackendStatus(next);
      } catch {
        if (active) setBackendStatus("failed");
      } finally {
        if (active) {
          timer = setTimeout(() => void check(), 500);
        }
      }
    }
    void check();
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, []);

  const handleBack = useCallback(() => {
    setAgentPhase("idle");
    setAgentMessage(null);
    void refreshConversations();
  }, [refreshConversations]);

  const providerChanged = useCallback(() => {
    setNeedsReconnect(false);
    setProviderRefreshKey((k) => k + 1);
  }, []);

  const handleProviderError = useCallback(() => {
    setNeedsReconnect(true);
  }, []);

  const bannerStatus: ProviderStatus | null = needsReconnect ? "needs-reconnect" : providerStatus;

  if (loading) {
    return (
      <main className="app app-shell" aria-busy="true">
        <p className="muted">{messages.app.loading}</p>
      </main>
    );
  }

  return (
    <main className="app app-shell">
      <header className="app-bar app-shell-header">
        <span className="app-title">{messages.app.title}</span>
        <button
          type="button"
          className="secondary app-settings-button"
          aria-label={messages.app.settings}
          aria-pressed={settingsOpen}
          onClick={() => setSettingsOpen((open) => !open)}
        >
          ⚙
        </button>
      </header>

      {bannerStatus && bannerStatus !== "free" && (
        <ProviderStatusBanner status={bannerStatus} onConnect={() => setSettingsOpen(true)} />
      )}

      <div className="app-body">
        <ConversationsSidebar
          conversations={conversations}
          selectedId={selectedId}
          onSelect={(id) => void openConversation(id)}
          onRefresh={refreshConversations}
        />

        {selectedId && conversation ? (
          <div className="conversation-main">
            <WorkspaceView
              project={conversation}
              agentPhase={agentPhase}
              agentMessage={agentMessage}
              onBack={handleBack}
              onRefresh={() => void refreshConversation(selectedId)}
              aiUsable={providerStatus !== "requires-choice" && backendStatus === "ready"}
              backendStatus={backendStatus}
              onRetryBackend={() => setBackendStatus("starting")}
              onOpenProvider={() => setSettingsOpen(true)}
              onProviderError={handleProviderError}
            />
          </div>
        ) : (
          <div className="conversation-placeholder">
            <p className="muted">{messages.app.loading}</p>
          </div>
        )}
      </div>

      {settingsOpen && (
        <ProviderPanel onClose={() => setSettingsOpen(false)} onChanged={providerChanged} />
      )}

      <ToastRegion toasts={toasts} />
    </main>
  );
}
