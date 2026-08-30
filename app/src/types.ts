export interface ProjectSummary {
  id: string;
  name: string;
}

export interface MaterialView {
  id: string;
  displayName: string;
  originalFileName: string;
  kind: string;
  byteSize: number;
}

export interface CreationView {
  id: string;
  displayName: string;
  kind: string;
  visibility: "public" | "private";
  byteSize: number;
}

export interface PublicationView {
  state: "local" | "published";
  publicUrl: string | null;
}

export interface ProjectView {
  id: string;
  name: string;
  materials: MaterialView[];
  creations: CreationView[];
  publication: PublicationView;
}

export interface AgentTaskEvent {
  projectId: string;
  status: "working" | "completed" | "failed" | "cancelled";
  message: string | null;
  registeredCreationIds: string[];
}

export interface AppError {
  code: string;
  message: string;
}

export type AgentPhase = "idle" | "working" | "completed" | "failed";
