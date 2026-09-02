import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import type {
  AgentTaskEvent,
  AppError,
  ConnectionTest,
  ConnectionView,
  CreationView,
  MaterialAddImageView,
  MaterialView,
  MaterialsImportReport,
  ModelSummary,
  OAuthAttempt,
  OAuthStatus,
  PreviewData,
  ProjectSummary,
  ProjectView,
  ProviderDetail,
  ProviderSummary,
  PublicationView,
  SelectedModelView,
  SessionLogEntry,
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
  materialAddImage: (projectId: string, fileName: string, contentType: string, data: Uint8Array) =>
    invoke<MaterialAddImageView>("material_add_image", {
      projectId,
      fileName,
      contentType,
      data: Array.from(data),
    }),
  materialsAddFromPaths: (projectId: string, paths: string[]) =>
    invoke<MaterialsImportReport>("materials_add_from_paths", { projectId, paths }),
  materialRemove: (projectId: string, materialId: string) =>
    invoke<void>("material_remove", { projectId, materialId }),
  materialOpen: (projectId: string, materialId: string) =>
    invoke<void>("material_open", { projectId, materialId }),
  materialOpenFolder: (projectId: string, materialId: string) =>
    invoke<void>("material_open_folder", { projectId, materialId }),
  previewData: (projectId: string, resourceKind: string, resourceId: string) =>
    invoke<PreviewData>("preview_data", { projectId, resourceKind, resourceId }),
  previewOpenWeb: (projectId: string, creationId: string) =>
    invoke<void>("preview_open_web", { projectId, creationId }),
  previewClose: (token: string) => invoke<void>("preview_close", { token }),
  creationSetVisibility: (projectId: string, creationId: string, isPublic: boolean) =>
    invoke<CreationView>("creation_set_visibility", {
      projectId,
      creationId,
      public: isPublic,
    }),
  creationOpen: (projectId: string, creationId: string) =>
    invoke<void>("creation_open", { projectId, creationId }),
  creationOpenFolder: (projectId: string, creationId: string) =>
    invoke<void>("creation_open_folder", { projectId, creationId }),
  openPublicUrl: (projectId: string) => invoke<void>("open_public_url", { projectId }),
  agentSend: (projectId: string, prompt: string, attachmentIds: string[] = []) =>
    invoke<void>("agent_send", { projectId, prompt, attachmentIds }),
  agentCancel: (projectId: string) => invoke<void>("agent_cancel", { projectId }),
  publish: (projectId: string, creationId?: string | null) =>
    invoke<PublicationView>("publish", { projectId, creationId: creationId ?? null }),
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

  // -- M7 provider/model surface ----------------------------------------------

  providerList: () => invoke<ProviderSummary[]>("provider_list"),
  providerDetail: (providerId: string) => invoke<ProviderDetail>("provider_detail", { providerId }),
  providerConnectKey: (providerId: string, key: string, label?: string | null) =>
    invoke<ConnectionView>("provider_connect_key", { providerId, key, label: label ?? null }),
  providerOauthBegin: (providerId: string, methodId: string) =>
    invoke<OAuthAttempt>("provider_oauth_begin", { providerId, methodId }),
  providerOauthStatus: (attemptId: string) =>
    invoke<OAuthStatus>("provider_oauth_status", { attemptId }),
  providerOauthComplete: (attemptId: string, code?: string | null) =>
    invoke<ConnectionView>("provider_oauth_complete", { attemptId, code: code ?? null }),
  providerOauthCancel: (attemptId: string) => invoke<void>("provider_oauth_cancel", { attemptId }),
  providerOauthOpen: (url: string) => invoke<void>("provider_oauth_open", { url }),
  providerDisconnect: (credentialId: string) =>
    invoke<void>("provider_disconnect", { credentialId }),
  providerTestConnection: (providerId: string, modelId?: string | null) =>
    invoke<ConnectionTest>("provider_test_connection", {
      providerId,
      modelId: modelId ?? null,
    }),
  modelList: () => invoke<ModelSummary[]>("model_list"),
  modelSelect: (providerId: string, modelId: string) =>
    invoke<void>("model_select", { providerId, modelId }),
  conversationModelSelect: (projectId: string, providerId: string, modelId: string) =>
    invoke<void>("conversation_model_select", { projectId, providerId, modelId }),
  conversationModelClear: (projectId: string) =>
    invoke<void>("conversation_model_clear", { projectId }),
  modelGetSelected: () => invoke<SelectedModelView>("model_get_selected"),
  sessionLogs: () => invoke<SessionLogEntry[]>("session_logs"),
  sessionLogsClear: () => invoke<void>("session_logs_clear"),
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
