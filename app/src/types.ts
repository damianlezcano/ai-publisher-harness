export interface ProjectSummary {
  id: string;
  name: string;
  createdAt: string;
  updatedAt: string;
  shared: boolean;
}

export interface MaterialView {
  id: string;
  displayName: string;
  originalFileName: string;
  kind: string;
  byteSize: number;
  createdAt: string;
}

export interface CreationView {
  id: string;
  displayName: string;
  kind: string;
  visibility: "public" | "private";
  byteSize: number;
  createdAt: string;
  revision: number;
}

export interface MaterialImportResult {
  sourceName: string;
  status: "added" | "duplicate" | "duplicate_in_batch" | "unsupported" | "failed";
  materialId?: string;
  reason?: string;
  material?: MaterialView;
}

export interface MaterialsImportReport {
  items: MaterialImportResult[];
}

export interface StagedAttachmentView {
  sourceName: string;
  status: "ready" | "duplicate_in_selection" | "unsupported" | "failed";
  reason?: string;
}

export interface StagedAttachmentsReport {
  items: StagedAttachmentView[];
}

export interface PreviewData {
  contentType: string;
  dataBase64: string;
}

export interface PublicationView {
  state: "local" | "published";
  publicUrl: string | null;
}

export interface MessageView {
  id: string;
  role: "user" | "assistant";
  text: string;
  status: "ok" | "failed" | "cancelled";
  createdAt: string;
  materialIds: string[];
  creationIds: string[];
}

export interface ProjectView {
  id: string;
  name: string;
  materials: MaterialView[];
  creations: CreationView[];
  publication: PublicationView;
  messages: MessageView[];
  model?: ConversationModelView | null;
  acceptedImport?: AcceptedImportProgressView | null;
}

export interface AcceptedImportProgressView {
  operationId: string;
  state: string;
  agentState: string;
  total: number;
  copied: number;
  lexicalCompleted: number;
  embeddingCompleted: number;
  failed: number;
  embeddingsCreated: number;
  embeddingsReused: number;
}

export interface ConversationModelView {
  providerId: string;
  modelId: string;
}

export interface AgentTaskEvent {
  projectId: string;
  turnId?: string;
  status: "working" | "completed" | "failed" | "cancelled";
  message: string | null;
  registeredCreationIds: string[];
}

export interface AppError {
  code: string;
  message: string;
}

export type AgentPhase = "idle" | "working" | "completed" | "failed";

export type BackendReadiness = "starting" | "ready" | "failed";

// -- M7 provider/model surface ------------------------------------------------

export type AuthMethodKind = "api_key" | "account";

export interface AuthPrompt {
  key: string;
  message: string;
  kind: "text" | "select";
  options: string[];
  placeholder: string | null;
  optional: boolean;
}

export interface AuthMethodView {
  kind: AuthMethodKind;
  methodId: string | null;
  label: string;
  prompts: AuthPrompt[];
}

export interface ConnectionView {
  id: string;
  label: string | null;
}

export interface ProviderSummary {
  id: string;
  name: string;
  authMethods: AuthMethodView[];
  connected: boolean;
  connectionLabel: string | null;
  highlighted: boolean;
}

export interface ProviderDetail {
  id: string;
  name: string;
  authMethods: AuthMethodView[];
  connections: ConnectionView[];
}

export interface ModelSummary {
  providerId: string;
  modelId: string;
  name: string;
  free: boolean;
  recommended: boolean;
  deprecated: boolean;
}

export type OAuthMode = "auto" | "code";
export type OAuthStatusKind = "pending" | "complete" | "failed" | "expired";

export interface OAuthAttempt {
  attemptId: string;
  url: string;
  instructions: string | null;
  mode: OAuthMode;
}

export interface OAuthStatus {
  status: OAuthStatusKind;
  message: string | null;
}

export type ConnectionTestOutcome =
  | "connected"
  | "credential_invalid"
  | "provider_unavailable"
  | "no_compatible_model"
  | "network_error";

export interface ConnectionTest {
  outcome: ConnectionTestOutcome;
  message: string;
}

export interface SelectedModelView {
  model: ModelSummary;
  notice: string | null;
  requiresChoice: boolean;
}

export interface SessionLogEntry {
  level: "ERROR" | "WARN" | "INFO" | "DEBUG" | string;
  message: string;
  usage?: SessionUsage | null;
  knowledge?: SessionKnowledgeMetrics | null;
}

export interface SessionUsage {
  conversationId: string;
  turnId: string;
  provider: string;
  model: string;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  totalTokens: number | null;
  costUsd: number | null;
  turnDurationMs: number | null;
  source: "provider_actual" | "estimated" | "unavailable" | string;
}

export interface SessionKnowledgeMetrics {
  conversationId: string;
  materialCount: number;
  corpusBytes: number;
  corpusUtf8Chars: number;
  corpusEstTokens: number;
  retrievalCandidateCount: number;
  selectedEvidenceCount: number;
  selectedEvidenceBytes: number;
  selectedEvidenceUtf8Chars: number;
  evidenceEstTokens: number;
  contextReductionPct: number;
  semanticProviderState: string;
  requestPreparationMs: number;
}

export interface SummarizationReportView {
  remoteCalls: number;
  estimatedInputUnits: number;
  cacheHits: number;
  reused: number;
  regenerated: number;
  sourceCount: number;
  hierarchyDepth: number;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheReadTokens?: number | null;
  cacheWriteTokens?: number | null;
  costUsd?: number | null;
  providerUsageActual?: boolean;
}

export interface ProjectSummaryAnswerView {
  report: SummarizationReportView;
  globalSummary: string | null;
}
