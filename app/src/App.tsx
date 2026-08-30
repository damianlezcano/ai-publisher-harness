import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "./api";
import type { AgentPhase, ProjectSummary, ProjectView } from "./types";
import ProjectsView from "./components/ProjectsView";
import WorkspaceView from "./components/WorkspaceView";

export default function App() {
  const [projects, setProjects] = useState<ProjectSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [project, setProject] = useState<ProjectView | null>(null);
  const [loading, setLoading] = useState(true);
  const [agentPhase, setAgentPhase] = useState<AgentPhase>("idle");
  const [agentMessage, setAgentMessage] = useState<string | null>(null);

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
          setAgentMessage(event.message ?? "Listo.");
          void refreshProject(selectedId).catch(() => {});
        } else {
          setAgentPhase("failed");
          setAgentMessage(event.message ?? "No se pudo completar la creación.");
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, [selectedId, refreshProject]);

  const openProject = useCallback(async (id: string) => {
    setSelectedId(id);
    setAgentPhase("idle");
    setAgentMessage(null);
    try {
      setProject(await api.projectOpen(id));
    } catch (error) {
      setProject(null);
      setAgentMessage(errorMessage(error));
    }
  }, []);

  const goBack = useCallback(() => {
    setSelectedId(null);
    setProject(null);
    setAgentPhase("idle");
    setAgentMessage(null);
    void refreshProjects();
  }, [refreshProjects]);

  if (loading) {
    return (
      <main className="app" aria-busy="true">
        <p className="muted">Cargando…</p>
      </main>
    );
  }

  return (
    <main className="app">
      {selectedId && project ? (
        <WorkspaceView
          project={project}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          onBack={goBack}
          onRefresh={() => void refreshProject(selectedId)}
        />
      ) : (
        <ProjectsView
          projects={projects}
          onRefresh={refreshProjects}
          onOpen={(id) => void openProject(id)}
        />
      )}
    </main>
  );
}
