import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ConversationDetails from "./ConversationDetails";
import type { ProjectView } from "../types";
import { humanDate, humanSize } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);

const project = (id: string, messages: ProjectView["messages"] = []): ProjectView => ({
  id,
  name: `Conversación ${id}`,
  materials: [],
  creations: [],
  messages,
  publication: { state: "local", publicUrl: null },
  model: null,
});

const accumulatedA = {
  conversationId: "a",
  provider: "Proveedor con nombre muy largo para comprobar el ajuste seguro",
  model: "modelo-grande-para-pruebas",
  inputTokens: 4321,
  outputTokens: 876,
  cacheReadTokens: 32,
  cacheWriteTokens: null,
  totalTokens: null,
  costUsd: 0.123,
  turnDurationMs: null,
  source: "provider_actual",
  remoteCalls: 2,
};

const accumulatedB = {
  ...accumulatedA,
  conversationId: "b",
  inputTokens: 77,
  outputTokens: 11,
  costUsd: 0.001,
  provider: "otro-proveedor",
  model: "modelo-chico",
};

const detailProject: ProjectView = {
  ...project("0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22"),
  name: "Fotosíntesis",
  materials: [
    {
      id: "m1",
      displayName: "manual.pdf",
      originalFileName: "manual.pdf",
      kind: "pdf",
      byteSize: 10,
      createdAt: "2026-08-28T15:00:00Z",
    },
  ],
  creations: [
    {
      id: "c1",
      displayName: "Actividad",
      kind: "web",
      visibility: "private",
      byteSize: 20,
      createdAt: "2026-08-28T15:00:00Z",
      revision: 1,
      lineageId: "c1",
      versionNumber: 1,
      isCurrent: true,
      availableVersionIds: ["c1"],
    },
  ],
};
const model = {
  providerId: "opencode",
  modelId: "big-pickle",
  name: "Big Pickle",
  free: true,
  recommended: true,
  deprecated: false,
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((command: string) => {
    if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
    if (command === "session_logs") return Promise.resolve([]);
    return Promise.resolve(undefined);
  });
});

describe("ConversationDetails metrics", () => {
  it("renders accumulated provider metrics from accumulated usage", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({
          provider: "durable-provider",
          model: "durable-model",
          inputTokens: 999999,
          outputTokens: 876,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: 0.999,
          turnDurationMs: 88,
          source: "provider_actual",
          remoteCalls: 2,
          materialCount: 4,
          corpusBytes: 9876,
          corpusUtf8Chars: 9000,
          corpusEstTokens: 2222,
          retrievalCandidateCount: 9,
          selectedEvidenceCount: 3,
          selectedEvidenceBytes: 444,
          selectedEvidenceUtf8Chars: 400,
          evidenceEstTokens: 111,
          contextReductionPct: 95,
          semanticProviderState: "available",
          requestPreparationMs: 12,
          retrievalMode: "normal",
          exhaustiveCoverage: "not_requested",
        });
      if (command === "conversation_accumulated_usage") return Promise.resolve(accumulatedA);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("4.321 tokens")).toBeVisible();
    expect(screen.getByText("USD 0.123")).toBeVisible();
    expect(screen.queryByText("999.999 tokens")).not.toBeInTheDocument();
  });

  it("shows accumulated Knowledge summary when Knowledge was used via durable messages", async () => {
    const knowledgeMessages = [
      {
        id: "u1",
        role: "user" as const,
        text: "pregunta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:00Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 3,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a1",
        role: "assistant" as const,
        text: "respuesta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:01Z",
        materialIds: [],
        creationIds: [],
        turnId: "u1",
      },
      {
        id: "u2",
        role: "user" as const,
        text: "otra",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:02Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 3,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: null,
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: "inventory",
        },
      },
      {
        id: "a2",
        role: "assistant" as const,
        text: "respuesta 2",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:03Z",
        materialIds: [],
        creationIds: [],
        turnId: "u2",
      },
    ];
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ materialCount: 3, retrievalMode: "normal" });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 1000 });
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a", knowledgeMessages)}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.getByText("Knowledge")).toBeVisible();
    expect(screen.getByText("Usado en 2 respuestas")).toBeVisible();
    expect(screen.getByText("Materiales utilizados")).toBeVisible();
    expect(screen.getByText("3")).toBeVisible();
  });

  it("shows Knowledge not used when no Knowledge responses exist in messages or logs", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ materialCount: 0 });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 100 });
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.getByText("No usado en esta conversación")).toBeVisible();
  });

  it("does not count failed, cancelled, or responseless turns toward Knowledge count", async () => {
    const knowledgeMessages = [
      {
        id: "u1",
        role: "user" as const,
        text: "pregunta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:00Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 3,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a1-failed",
        role: "assistant" as const,
        text: "error",
        status: "failed" as const,
        createdAt: "2026-09-20T10:00:01Z",
        materialIds: [],
        creationIds: [],
        turnId: "u1",
      },
      {
        id: "u2",
        role: "user" as const,
        text: "otra",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:02Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 3,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: null,
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a2-cancelled",
        role: "assistant" as const,
        text: "",
        status: "cancelled" as const,
        createdAt: "2026-09-20T10:00:03Z",
        materialIds: [],
        creationIds: [],
        turnId: "u2",
      },
      {
        id: "u3",
        role: "user" as const,
        text: "tercera",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:04Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 3,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      // u3 has no assistant response at all — should not be counted
    ];
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ materialCount: 3, retrievalMode: "normal" });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 500 });
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a", knowledgeMessages)}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    // Only u1 (failed→not counted), u2 (cancelled→not counted), u3 (no response→not counted)
    // None have a successful linked response, so count is 0.
    expect(screen.getByText("No usado en esta conversación")).toBeVisible();
  });

  it("preserves durable Knowledge count across reload (logs empty but messages persist)", async () => {
    const knowledgeMessages = [
      {
        id: "u1",
        role: "user" as const,
        text: "pregunta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:00Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 2,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a1",
        role: "assistant" as const,
        text: "respuesta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:01Z",
        materialIds: [],
        creationIds: [],
        turnId: "u1",
      },
    ];
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ materialCount: 2, retrievalMode: "normal" });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 500 });
      if (command === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a", knowledgeMessages)}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.getByText("Usado en 1 respuesta")).toBeVisible();
    expect(screen.queryByText("No usado en esta conversación")).not.toBeInTheDocument();
  });

  it("preserves name, model, resource rows, folder actions, and close behavior", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list" || command === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const onRefresh = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(
      <ConversationDetails
        project={detailProject}
        active={false}
        onClose={onClose}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByLabelText("Nombre")).toHaveValue("Fotosíntesis");
    expect(await screen.findByRole("option", { name: /Big Pickle/ })).toBeInTheDocument();
    const materialRow = screen.getByText("manual.pdf").closest("li");
    expect(materialRow).toHaveClass("item-row");
    expect(materialRow?.textContent).toContain(humanSize(10));
    expect(materialRow?.textContent).toContain(humanDate(detailProject.materials[0].createdAt));
    fireEvent.change(screen.getByLabelText("Nombre"), { target: { value: "Nueva" } });
    await user.click(screen.getByRole("button", { name: "Renombrar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_rename", {
        projectId: detailProject.id,
        name: "Nueva",
      }),
    );
    await user.selectOptions(
      screen.getByLabelText("Modelo de esta conversación"),
      "opencode::big-pickle",
    );
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_select", {
      projectId: detailProject.id,
      providerId: "opencode",
      modelId: "big-pickle",
    });
    const folders = screen.getAllByRole("button", { name: "Abrir carpeta contenedora" });
    await user.click(folders[0]);
    await user.click(folders[1]);
    expect(invokeMock).toHaveBeenCalledWith("materials_open_folder", {
      projectId: detailProject.id,
    });
    expect(invokeMock).toHaveBeenCalledWith("creations_open_folder", {
      projectId: detailProject.id,
    });
    await user.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("preserves safe text and image previews and active-model locking", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list" || command === "session_logs") return Promise.resolve([]);
      if (command === "preview_data")
        return Promise.resolve({ contentType: "text/markdown", dataBase64: btoa("<b>Hola</b>") });
      return Promise.resolve(undefined);
    });
    const user = userEvent.setup();
    const view = render(
      <ConversationDetails
        project={detailProject}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abrir: manual.pdf" }));
    expect(
      (await screen.findByRole("dialog", { name: "manual.pdf" })).querySelector("pre"),
    ).toBeInTheDocument();
    view.unmount();
    render(
      <ConversationDetails
        project={{ ...detailProject, model: { providerId: "opencode", modelId: "big-pickle" } }}
        active
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByLabelText("Modelo de esta conversación")).toBeDisabled();
  });

  it("renders missing provider fields as No disponible rather than zero", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({
          ...accumulatedA,
          inputTokens: 31,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: "estimated",
        });
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await waitFor(() =>
      expect(screen.getAllByText("No disponible").length).toBeGreaterThanOrEqual(6),
    );
    expect(screen.queryByText("0 tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("31 tokens")).not.toBeInTheDocument();
  });

  it("does not show last-turn metrics section", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_accumulated_usage") return Promise.resolve(accumulatedA);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.queryByRole("heading", { name: "Último turno" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Uso real del proveedor" }),
    ).not.toBeInTheDocument();
  });

  it("does not show retrieval mode, candidates, or evidence in the detail surface", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_accumulated_usage") return Promise.resolve(accumulatedA);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.queryByText("Modo de recuperación")).not.toBeInTheDocument();
    expect(screen.queryByText("Candidatos de recuperación")).not.toBeInTheDocument();
    expect(screen.queryByText("Evidencias seleccionadas")).not.toBeInTheDocument();
    expect(screen.queryByText("Corpus estimado")).not.toBeInTheDocument();
    expect(screen.queryByText("Reducción estimada de contexto")).not.toBeInTheDocument();
  });

  it("uses the selected conversation accumulated usage only, not a stale other conversation", async () => {
    invokeMock.mockImplementation((command: string, args?: unknown) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_accumulated_usage") {
        const a = args as Record<string, unknown> | undefined;
        const pid = a?.projectId as string;
        return Promise.resolve(pid === "b" ? accumulatedB : accumulatedA);
      }
      return Promise.resolve(undefined);
    });
    const view = render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("4.321 tokens")).toBeVisible();
    expect(screen.queryByText("otro-proveedor")).not.toBeInTheDocument();
    view.rerender(
      <ConversationDetails
        project={project("b")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("77 tokens")).toBeVisible();
    expect(screen.getByText("otro-proveedor")).toBeVisible();
    expect(screen.queryByText("4.321 tokens")).not.toBeInTheDocument();
  });

  it("shows Knowledge summary with distinct dt/dd labels for accessibility", async () => {
    const knowledgeMessages = [
      {
        id: "u1",
        role: "user" as const,
        text: "pregunta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:00Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: 2,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a1",
        role: "assistant" as const,
        text: "respuesta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:01Z",
        materialIds: [],
        creationIds: [],
        turnId: "u1",
      },
    ];
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ materialCount: 2, retrievalMode: "normal" });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 500 });
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a", knowledgeMessages)}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    // dt and dd must have distinct content: label is "Knowledge", value is "Usado en 1 respuesta"
    const knowledgeLabel = screen.getByText("Knowledge");
    const knowledgeDd = knowledgeLabel.closest("div")?.querySelector("dd");
    expect(knowledgeDd).toHaveTextContent("Usado en 1 respuesta");
    // dt and dd must have distinct content: label is "Materiales utilizados", value is "2"
    const materialsLabel = screen.getByText("Materiales utilizados");
    const materialsDd = materialsLabel.closest("div")?.querySelector("dd");
    expect(materialsDd).toHaveTextContent("2");
  });

  it("shows No disponible for material count when durable metrics lack materialCount", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({ retrievalMode: "normal" });
      if (command === "conversation_accumulated_usage")
        return Promise.resolve({ ...accumulatedA, inputTokens: 500 });
      return Promise.resolve(undefined);
    });
    const knowledgeMessages = [
      {
        id: "u1",
        role: "user" as const,
        text: "pregunta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:00Z",
        materialIds: [],
        creationIds: [],
        turnMetrics: {
          provider: "p",
          model: "m",
          inputTokens: null,
          outputTokens: null,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: null,
          turnDurationMs: null,
          source: null,
          remoteCalls: null,
          materialCount: null,
          corpusBytes: null,
          corpusUtf8Chars: null,
          corpusEstTokens: null,
          retrievalCandidateCount: null,
          selectedEvidenceCount: null,
          selectedEvidenceBytes: null,
          selectedEvidenceUtf8Chars: null,
          evidenceEstTokens: null,
          contextReductionPct: null,
          semanticProviderState: null,
          requestPreparationMs: null,
          retrievalMode: "normal",
          eligibleMaterials: null,
          materialsInspected: null,
          chunksInspected: null,
          exhaustiveCoverage: null,
          lexicalHits: null,
          semanticHits: null,
          localMode: null,
        },
      },
      {
        id: "a1",
        role: "assistant" as const,
        text: "respuesta",
        status: "ok" as const,
        createdAt: "2026-09-20T10:00:01Z",
        materialIds: [],
        creationIds: [],
        turnId: "u1",
      },
    ];
    render(
      <ConversationDetails
        project={project("a", knowledgeMessages)}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Uso y optimización" });
    const materialsLabel = screen.getByText("Materiales utilizados");
    const materialsDd = materialsLabel.closest("div")?.querySelector("dd");
    expect(materialsDd).toHaveTextContent("No disponible");
  });
});
