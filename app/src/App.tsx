import { useCallback, useEffect, useState } from "react";
import { api } from "./api";
import type { AgentPhase, ProjectSummary, ProjectView } from "./types";
import { guidanceFromError } from "./guidance";
import ProjectsView from "./components/ProjectsView";
import WorkspaceView from "./components/WorkspaceView";
import ModelSelector from "./components/provider/ModelSelector";
import ProviderPanel from "./components/provider/ProviderPanel";
import ProviderStatusBanner from "./components/ui/ProviderStatusBanner";
import type { ProviderStatus } from "./components/ui/ProviderStatusBanner";
import ToastRegion from "./components/ui/ToastRegion";
import { useToast } from "./components/ui/useToast";
import { messages } from "./messages";

export default function App() {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [project, setProject] = useState<ProjectView | null>(null);
  const [loading, setLoading] = useState(true);
  const [agentPhase, setAgentPhase] = useState<AgentPhase>("idle");
  const [agentMessage, setAgentMessage] = useState<string | null>(null);
  const [providerOpen, setProviderOpen] = useState(false);
  const [providerRefreshKey, setProviderRefreshKey] = useState(0);
  const [providerStatus, setProviderStatus] = useState<ProviderStatus | null>(null);
  const [needsReconnect, setNeedsReconnect] = useState(false);
  const { toasts, show } = useToast();

  const refreshProjects = useCallback(async () => {
    setProjects(await api.projectList());
  }, []);

  const refreshProject = useCallback(async (id: string) => {
    setProject(await api.projectOpen(id));
  }, []);

  useEffect(() => {
    void (async () => {
      try {
        await refreshProjects();
      } catch {
        // The projects view surfaces its own load error on retry.
      } finally {
        setLoading(false);
      }
    })();
  }, [refreshProjects]);

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
      })
      .then((fn) => {
        if (active) unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [selectedId, refreshProject, show]);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const current = await api.modelGetSelected();
        const models = await api.modelList();
        if (!active) return;
        const freeAvailable = current.model?.free === true || models.some((m) => m.free);
        setProviderStatus(
          current.requiresChoice ? "requires-choice" : freeAvailable ? "free" : null,
        );
      } catch {
        if (active) setProviderStatus(null);
      }
    })();
    return () => {
      active = false;
    };
  }, [providerRefreshKey]);

  const openProject = useCallback(
    async (id: string) => {
      setSelectedId(id);
      setAgentPhase("idle");
      setAgentMessage(null);
      try {
        setProject(await api.projectOpen(id));
      } catch (error) {
        setProject(null);
        show(guidanceFromError(error).title);
      }
    },
    [show],
  );

  const goBack = useCallback(() => {
    setSelectedId(null);
    setProject(null);
    setAgentPhase("idle");
    setAgentMessage(null);
    void refreshProjects();
  }, [refreshProjects]);

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
      <main className="app" aria-busy="true">
        <p className="muted">{messages.app.loading}</p>
      </main>
    );
  }

  return (
    <main className="app">
      <header className="app-bar">
        <span className="app-title">{messages.app.title}</span>
        <ModelSelector refreshKey={providerRefreshKey} />
        <button type="button" className="secondary" onClick={() => setProviderOpen(true)}>
          {messages.app.connectAi}
        </button>
      </header>

      {bannerStatus && (
        <ProviderStatusBanner status={bannerStatus} onConnect={() => setProviderOpen(true)} />
      )}

      {providerOpen && (
        <ProviderPanel onClose={() => setProviderOpen(false)} onChanged={providerChanged} />
      )}

      {selectedId && project ? (
        <WorkspaceView
          project={project}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          onBack={goBack}
          onRefresh={() => void refreshProject(selectedId)}
          aiUsable={providerStatus !== "requires-choice"}
          onOpenProvider={() => setProviderOpen(true)}
          onProviderError={handleProviderError}
        />
      ) : (
        <ProjectsView
          projects={projects}
          onRefresh={refreshProjects}
          onOpen={(id) => void openProject(id)}
        />
      )}

      <ToastRegion toasts={toasts} />
    </main>
  );
}
