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
  lineageId: string;
  versionNumber: number;
  parentVersionId?: string | null;
  isCurrent: boolean;
  availableVersionIds: string[];
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
  /**
   * Authoritative share URL when published. For a shared web lineage this is
   * the immutable current-version URL (/{slug}/{current-version-id}/); for
   * non-web publications it is the route root. Copy / Open / QR all consume
   * exactly this value.
   */
  publicUrl: string | null;
  /** Version-history landing page URL (/{slug}/). */
  rootUrl?: string | null;
  /** Current-version alias URL (/{slug}/latest/). */
  latestUrl?: string | null;
  /** The immutable version id the share URL currently targets. */
  currentVersionId?: string | null;
}

export interface TurnMetrics {
  provider: string | null;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  totalTokens: number | null;
  costUsd: number | null;
  turnDurationMs: number | null;
  source: string | null;
  remoteCalls: number | null;
  // Knowledge structural metrics
  materialCount: number | null;
  corpusBytes: number | null;
  corpusUtf8Chars: number | null;
  corpusEstTokens: number | null;
  retrievalCandidateCount: number | null;
  selectedEvidenceCount: number | null;
  selectedEvidenceBytes: number | null;
  selectedEvidenceUtf8Chars: number | null;
  evidenceEstTokens: number | null;
  contextReductionPct: number | null;
  semanticProviderState: string | null;
  requestPreparationMs: number | null;
  retrievalMode: string | null;
  eligibleMaterials: number | null;
  materialsInspected: number | null;
  chunksInspected: number | null;
  exhaustiveCoverage: string | null;
  lexicalHits: number | null;
  semanticHits: number | null;
  localMode: string | null;
  /** Exact grounded source display names for this turn (never filesystem paths). */
  sourceNames?: string[];
}

export interface ConversationUsageTotals {
  provider: string | null;
  model: string | null;
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheWriteTokens: number | null;
  totalTokens: number | null;
  costUsd: number | null;
  turnDurationMs: number | null;
  source: string | null;
  remoteCalls: number | null;
}

export interface MessageView {
  id: string;
  role: "user" | "assistant";
  text: string;
  status: "ok" | "failed" | "cancelled";
  createdAt: string;
  materialIds: string[];
  creationIds: string[];
  turnMetrics?: TurnMetrics | null;
  /** Durable id of the owning user turn (the user message id). Present on
   * assistant messages; absent on user messages and legacy records. */
  turnId?: string | null;
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
  summaryRetryable?: boolean;
  total: number;
  copied: number;
  lexicalCompleted: number;
  embeddingCompleted: number;
  failed: number;
  embeddingsCreated: number;
  embeddingsReused: number;
  chunksTotal: number;
  embeddingsTotal: number;
  /** Fully usable materials for the active Knowledge embedding generation. */
  materialsReady: number;
  elapsedMs: number;
  throughputEmbeddingsPerSec?: number | null;
  /** True while the turn's post-embedding compact/generic per-item summary
   * synthesis is running. Replaces the misleading "99% · N de N" import line
   * with a truthful synthesis phase. */
  synthesizing?: boolean;
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
  code?: string | null;
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
  promptContext?: PromptContextMetrics | null;
}

export interface PromptContextMetrics {
  conversationId: string;
  turnId: string;
  userPromptEstTokens: number;
  conversationHistoryEstTokens: number;
  knowledgeContextEstTokens: number;
  systemContextEstTokens: number;
  /** Bounded Knowledge evidence entries serialized in the provider request. */
  ragAttachmentCount: number;
  rawAttachmentCount: number;
  serializedRequestEstTokens: number;
  freshSession: boolean;
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
  remoteCalls?: number | null;
  reason?: string;
  additionalAttachmentRoute?: boolean;
}

export interface SessionKnowledgeMetrics {
  conversationId: string;
  materialCount: number;
  corpusBytes: number;
  corpusUtf8Chars: number;
  corpusEstTokens: number;
  retrievalCandidateCount: number | null;
  selectedEvidenceCount: number | null;
  selectedEvidenceBytes: number | null;
  selectedEvidenceUtf8Chars: number | null;
  evidenceEstTokens: number | null;
  contextReductionPct: number | null;
  semanticProviderState: string;
  requestPreparationMs: number | null;
  retrievalMode?: string | null;
  eligibleMaterials?: number | null;
  materialsInspected?: number | null;
  chunksInspected?: number | null;
  exhaustiveCoverage?: string | null;
  lexicalHits?: number | null;
  semanticHits?: number | null;
  localMode?: string | null;
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
