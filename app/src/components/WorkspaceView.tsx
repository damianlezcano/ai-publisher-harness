import type { AgentPhase, ProjectView } from "../types";
import ChatPanel from "./ChatPanel";
import CreationsPanel from "./CreationsPanel";
import MaterialsPanel from "./MaterialsPanel";
import PublishPanel from "./PublishPanel";

interface WorkspaceViewProps {
  project: ProjectView;
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onBack: () => void;
  onRefresh: () => void;
}

export default function WorkspaceView({
  project,
  agentPhase,
  agentMessage,
  onBack,
  onRefresh,
}: WorkspaceViewProps) {
  return (
    <div className="view workspace">
      <header className="view-header">
        <button type="button" className="secondary" onClick={onBack}>
          ← Proyectos
        </button>
        <h1>{project.name}</h1>
      </header>

      <div className="workspace-grid">
        <ChatPanel
          projectId={project.id}
          materials={project.materials}
          agentPhase={agentPhase}
          agentMessage={agentMessage}
          onRefresh={onRefresh}
        />
        <MaterialsPanel
          projectId={project.id}
          materials={project.materials}
          onRefresh={onRefresh}
        />
        <CreationsPanel
          projectId={project.id}
          creations={project.creations}
          onRefresh={onRefresh}
        />
        <PublishPanel
          projectId={project.id}
          publication={project.publication}
          onRefresh={onRefresh}
        />
      </div>
    </div>
  );
}
