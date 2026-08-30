import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  AgentTaskEvent,
  AppError,
  CreationView,
  MaterialView,
  ProjectSummary,
  ProjectView,
  PublicationView,
} from "./types";

export const api = {
  projectList: () => invoke<ProjectSummary[]>("project_list"),
  projectCreate: (name: string) => invoke<ProjectSummary>("project_create", { name }),
  projectOpen: (projectId: string) => invoke<ProjectView>("project_open", { projectId }),
  projectRename: (projectId: string, name: string) =>
    invoke<ProjectSummary>("project_rename", { projectId, name }),
  projectDelete: (projectId: string) => invoke<void>("project_delete", { projectId }),
  materialAddFromPath: (projectId: string, path: string) =>
    invoke<MaterialView>("material_add_from_path", { projectId, path }),
  creationSetVisibility: (projectId: string, creationId: string, isPublic: boolean) =>
    invoke<CreationView>("creation_set_visibility", {
      projectId,
      creationId,
      public: isPublic,
    }),
  creationOpen: (projectId: string, creationId: string) =>
    invoke<void>("creation_open", { projectId, creationId }),
  openPublicUrl: (projectId: string) => invoke<void>("open_public_url", { projectId }),
  agentSend: (projectId: string, prompt: string) =>
    invoke<void>("agent_send", { projectId, prompt }),
  agentCancel: (projectId: string) => invoke<void>("agent_cancel", { projectId }),
  publish: (projectId: string) => invoke<PublicationView>("publish", { projectId }),
  unpublish: (projectId: string) => invoke<PublicationView>("unpublish", { projectId }),
  publicationStatus: (projectId: string) =>
    invoke<PublicationView>("publication_status", { projectId }),
  appStatus: () => invoke<{ version: string; agent: string }>("app_status"),
  onAgentTask: (handler: (event: AgentTaskEvent) => void): Promise<UnlistenFn> =>
    listen<AgentTaskEvent>("agent://task", (event) => handler(event.payload)),
  pickFile: async (): Promise<string | null> => {
    const selected = await openDialog({ multiple: false, directory: false });
    return typeof selected === "string" ? selected : null;
  },
};

export function isAppError(value: unknown): value is AppError {
  return (
    typeof value === "object" &&
    value !== null &&
    "message" in value &&
    typeof (value as AppError).message === "string"
  );
}

export function errorMessage(error: unknown): string {
  if (isAppError(error)) return error.message;
  return "Algo salió mal. Inténtalo de nuevo.";
}
