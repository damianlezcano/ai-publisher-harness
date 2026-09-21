import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import WorkspaceView from "./WorkspaceView";
import { embeddingProgressPercent } from "../importProgress";
import { messages } from "../messages";
import type {
  AcceptedImportProgressView,
  CreationView,
  MaterialView,
  MessageView,
  ProjectView,
  StagedAttachmentsReport,
} from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const getCurrentWebviewMock = vi.mocked(getCurrentWebview);
const openDialogMock = vi.mocked(openDialog);

function mockDragDrop(): {
  dropHandler: (event: { payload: { type: string; paths?: string[] } }) => void;
} {
  let dropHandler: ((event: { payload: { type: string; paths?: string[] } }) => void) | undefined;
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockImplementation((handler) => {
      dropHandler = handler;
      return Promise.resolve(() => {});
    }),
  } as never);
  return {
    dropHandler: (event) => dropHandler?.(event),
  };
}

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials: MaterialView[] = [
  {
    id: "m1",
    displayName: "diagrama.png",
    originalFileName: "diagrama.png",
    kind: "image",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
  },
  {
    id: "m2",
    displayName: "manual.pdf",
    originalFileName: "manual.pdf",
    kind: "pdf",
    byteSize: 2048,
    createdAt: "2026-08-28T15:00:00Z",
  },
];

const creations: CreationView[] = [
  {
    id: "c1",
    displayName: "actividad",
    kind: "web",
    visibility: "private",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
    revision: 1,
    lineageId: "c1",
    versionNumber: 1,
    isCurrent: true,
    availableVersionIds: ["c1"],
  },
];

const messagesList: MessageView[] = [
  {
    id: "msg-1",
    role: "user",
    text: "Creá algo",
    status: "ok",
    createdAt: "2026-08-28T15:00:00Z",
    materialIds: ["m1"],
    creationIds: [],
  },
  {
    id: "msg-2",
    role: "assistant",
    text: "Acá está",
    status: "ok",
    createdAt: "2026-08-28T15:01:00Z",
    materialIds: [],
    creationIds: ["c1"],
  },
  {
    id: "msg-3",
    role: "assistant",
    text: "No se pudo.",
    status: "failed",
    createdAt: "2026-08-28T15:02:00Z",
    materialIds: [],
    creationIds: [],
  },
];

function makeProject(extraMaterials?: MaterialView[]): ProjectView {
  return {
    id: projectId,
    name: "Fotosíntesis",
    materials: extraMaterials ? [...materials, ...extraMaterials] : materials,
    creations,
    publication: { state: "local", publicUrl: null },
    messages: messagesList,
  };
}

function makeAcceptedImport(
  acceptedImport: Partial<NonNullable<ProjectView["acceptedImport"]>>,
): ProjectView {
  const full: NonNullable<ProjectView["acceptedImport"]> = {
    operationId: acceptedImport.operationId ?? "op-1",
    state: acceptedImport.state ?? "indexing_lexical",
    agentState: acceptedImport.agentState ?? "not_started",
    total: acceptedImport.total ?? 0,
    copied: acceptedImport.copied ?? 0,
    lexicalCompleted: acceptedImport.lexicalCompleted ?? 0,
    embeddingCompleted: acceptedImport.embeddingCompleted ?? 0,
    failed: acceptedImport.failed ?? 0,
    embeddingsCreated: acceptedImport.embeddingsCreated ?? 0,
    embeddingsReused: acceptedImport.embeddingsReused ?? 0,
    chunksTotal: acceptedImport.chunksTotal ?? 0,
    embeddingsTotal: acceptedImport.embeddingsTotal ?? 0,
    materialsReady: acceptedImport.materialsReady ?? 0,
    elapsedMs: acceptedImport.elapsedMs ?? 0,
    throughputEmbeddingsPerSec: acceptedImport.throughputEmbeddingsPerSec ?? null,
    summaryRetryable: acceptedImport.summaryRetryable ?? false,
    synthesizing: acceptedImport.synthesizing ?? false,
  };
  return { ...makeProject(), acceptedImport: full };
}

const baseProps = {
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onBack: vi.fn(),
  onRefresh: vi.fn(),
  aiUsable: true,
  backendStatus: "ready" as const,
  onOpenProvider: vi.fn(),
  onProviderError: vi.fn(),
  onRetryBackend: vi.fn(),
  onSendEnd: vi.fn(),
};

function setupApi(
  options: {
    agentSendResult?: unknown;
    agentSendError?: unknown;
    agentSendPromise?: Promise<unknown>;
    stagedReport?: StagedAttachmentsReport;
  } = {},
) {
  const { agentSendResult = undefined, agentSendError, agentSendPromise, stagedReport } = options;
  invokeMock.mockImplementation((cmd: string, args?: unknown) => {
    switch (cmd) {
      case "model_list":
        return Promise.resolve([
          {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "Big Pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
        ]);
      case "provider_list":
        return Promise.resolve([]);
      case "model_get_selected":
        return Promise.resolve({
          model: {
            providerId: "opencode",
            modelId: "big-pickle",
            name: "Big Pickle",
            free: true,
            recommended: true,
            deprecated: false,
          },
          notice: null,
          requiresChoice: false,
        });
      case "agent_send_staged":
        if (agentSendError) return Promise.reject(agentSendError);
        if (agentSendPromise) return agentSendPromise;
        return Promise.resolve(agentSendResult);
      case "attachments_stage_paths":
        return Promise.resolve(
          stagedReport ?? {
            items: ((args as { paths?: string[] } | undefined)?.paths ?? []).map((path) => ({
              sourceName: path.split("/").at(-1) ?? "archivo",
              status: "ready" as const,
            })),
          },
        );
      default:
        return Promise.resolve(undefined);
    }
  });
}

describe("embeddingProgressPercent", () => {
  const progress = (overrides: Partial<AcceptedImportProgressView>) => ({
    operationId: "durable-operation",
    state: "indexing_embeddings",
    agentState: "not_started",
    total: 50,
    copied: 50,
    lexicalCompleted: 50,
    embeddingCompleted: 0,
    failed: 0,
    embeddingsCreated: 0,
    embeddingsReused: 0,
    chunksTotal: 100,
    embeddingsTotal: 100,
    materialsReady: 0,
    elapsedMs: 0,
    ...overrides,
  });

  it("derives bounded monotonic progress from durable embedding counters", () => {
    const values = [0, 27, 84, 100].map((embeddingCompleted) =>
      embeddingProgressPercent(progress({ embeddingCompleted })),
    );
    expect(values).toEqual([0, 27, 84, 99]);
    expect(values.every((value, index) => index === 0 || value! >= values[index - 1]!)).toBe(true);
  });

  it("keeps file readiness separate and reconstructs the same value after reopen", () => {
    const intermediate = progress({ embeddingCompleted: 54, materialsReady: 0 });
    const partialReady = progress({ embeddingCompleted: 84, materialsReady: 17 });
    const reopened = JSON.parse(JSON.stringify(partialReady)) as AcceptedImportProgressView;
    expect(embeddingProgressPercent(intermediate)).toBe(54);
    expect(intermediate.materialsReady).toBe(0);
    expect(embeddingProgressPercent(partialReady)).toBe(84);
    expect(embeddingProgressPercent(reopened)).toBe(84);
    expect(reopened.materialsReady).toBe(17);
  });

  it("handles zero totals and only permits 100 for a terminal operation", () => {
    expect(embeddingProgressPercent(progress({ embeddingsTotal: 0 }))).toBeNull();
    expect(embeddingProgressPercent(progress({ embeddingCompleted: 100 }))).toBe(99);
    expect(
      embeddingProgressPercent(
        progress({ state: "completed", embeddingCompleted: 100, materialsReady: 50 }),
      ),
    ).toBe(100);
  });
});

beforeEach(() => {
  invokeMock.mockReset();
  openDialogMock.mockReset();
  baseProps.onRefresh.mockReset();
  baseProps.onOpenProvider.mockReset();
  baseProps.onProviderError.mockReset();
  baseProps.onSendEnd.mockReset();
  baseProps.onBack.mockReset();
  getCurrentWebviewMock.mockReset();
  getCurrentWebviewMock.mockReturnValue({
    onDragDropEvent: vi.fn().mockResolvedValue(() => {}),
  } as never);
});

describe("WorkspaceView", () => {
  it("opens conversation details from the title and returns to the workspace on close", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    await userEvent.click(screen.getByRole("button", { name: "Detalles de Fotosíntesis" }));
    expect(
      await screen.findByRole("dialog", { name: "Detalles de la conversación" }),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument();
  });

  it("renders the project name as an h1", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByRole("heading", { name: "Fotosíntesis" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: messages.project.backToList }),
    ).not.toBeInTheDocument();
  });

  it("renders user and assistant messages from project.messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByText("Creá algo")).toBeInTheDocument();
    expect(screen.getByText("Acá está")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.userLabel)).toBeInTheDocument();
    expect(screen.getAllByText(messages.timeline.assistantLabel).length).toBeGreaterThan(0);
  });

  it("renders a failed assistant message as an error alert", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    const alerts = screen.getAllByRole("alert");
    expect(alerts.some((alert) => alert.textContent?.includes("No se pudo."))).toBe(true);
  });

  it("renders material chips on user messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
  });

  it("renders inline creation cards on assistant messages", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.getByText(creations[0].displayName)).toBeInTheDocument();
  });

  it("stages a file locally, then sends its path through the accepted-turn boundary", async () => {
    setupApi();
    openDialogMock.mockResolvedValueOnce("/tmp/diagrama.png");
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: messages.assistant.attachMaterial }));

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send_staged", {
        projectId,
        prompt: "Creá una actividad",
        stagedPaths: ["/tmp/diagrama.png"],
      }),
    );
    expect(baseProps.onRefresh).toHaveBeenCalled();
  });

  it("sends a fully quoted prompt verbatim as ordinary text", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, '"hola"');
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    // User text is data, not syntax: the exact quoted string (quotes included)
    // must be handed to the backend, never stripped or re-quoted.
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send_staged", {
        projectId,
        prompt: '"hola"',
        stagedPaths: [],
      }),
    );
  });

  it("sends shell-like user text verbatim without executing it", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "$(touch /tmp/educai-should-not-exist)");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send_staged", {
        projectId,
        prompt: "$(touch /tmp/educai-should-not-exist)",
        stagedPaths: [],
      }),
    );
  });

  it("uses an attached image as turn input without opening any preview", async () => {
    setupApi();
    openDialogMock.mockResolvedValueOnce("/tmp/diagrama.png");
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: messages.assistant.attachMaterial }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument(),
    );

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "agregá esta imagen en el encabezado");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send_staged", {
        projectId,
        prompt: "agregá esta imagen en el encabezado",
        stagedPaths: ["/tmp/diagrama.png"],
      }),
    );
    // The turn input flow must never trigger the manual preview/open actions.
    expect(invokeMock).not.toHaveBeenCalledWith(
      "material_open",
      expect.objectContaining({ projectId }),
    );
    expect(invokeMock).not.toHaveBeenCalledWith(
      "preview_data",
      expect.objectContaining({ projectId }),
    );
  });

  it("clears the draft after send and never re-suggests the previous attachment", async () => {
    setupApi();
    openDialogMock.mockResolvedValueOnce("/tmp/diagrama.png");
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    await userEvent.click(screen.getByRole("button", { name: messages.assistant.attachMaterial }));
    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument(),
    );

    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "agent_send_staged",
        expect.objectContaining({ projectId }),
      ),
    );

    // After send the composer draft is clean: no chip from the previous turn and
    // the attach action does not open a stale material suggestion list.
    expect(screen.queryByRole("button", { name: "Quitar diagrama.png" })).not.toBeInTheDocument();
    expect(screen.queryByText(messages.material.addFile)).not.toBeInTheDocument();
  });

  it("does not start a second agent turn while one send is already in flight", async () => {
    let release!: () => void;
    const hung = new Promise<void>((resolve) => {
      release = resolve;
    });
    setupApi({ agentSendPromise: hung });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "primero");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));
    await waitFor(() =>
      expect(invokeMock.mock.calls.filter((call) => call[0] === "agent_send_staged")).toHaveLength(
        1,
      ),
    );

    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "segundo");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));
    expect(invokeMock.mock.calls.filter((call) => call[0] === "agent_send_staged")).toHaveLength(1);

    await act(async () => {
      release();
    });
    await waitFor(() => expect(baseProps.onRefresh).toHaveBeenCalled());
  });

  it("calls onProviderError when a send error guides to connect-ai", async () => {
    setupApi({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Creá algo");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() => expect(baseProps.onProviderError).toHaveBeenCalledTimes(1));
    expect(baseProps.onSendEnd).toHaveBeenCalledWith(projectId);
  });

  it("clears the optimistic pending bubble after a send error", async () => {
    setupApi({ agentSendError: { code: "credential_revoked", message: "raw" } });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Este mensaje falla");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() => expect(baseProps.onProviderError).toHaveBeenCalledTimes(1));
    expect(
      screen.queryByText("Este mensaje falla", { selector: ".chat-log" }),
    ).not.toBeInTheDocument();
  });

  it("does not restore the submitted draft when only the post-accept refresh fails", async () => {
    baseProps.onRefresh.mockRejectedValueOnce({ code: "open_failed", message: "raw" });
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "mensaje único");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    // The turn is accepted by agentSendStaged exactly once; the refresh that
    // follows is the only failing step.
    await waitFor(() =>
      expect(invokeMock.mock.calls.filter((call) => call[0] === "agent_send_staged")).toHaveLength(
        1,
      ),
    );
    await waitFor(() => expect(baseProps.onRefresh).toHaveBeenCalled());

    // Accepted message is committed: the composer stays empty and the submitted
    // text must not be restored by the refresh failure.
    expect(screen.getByLabelText("Pedido a la IA")).toHaveValue("");
    expect(screen.queryByDisplayValue("mensaje único")).not.toBeInTheDocument();

    // No duplicate send is invited: agentSendStaged must not run again and no
    // resend action may be offered.
    expect(invokeMock.mock.calls.filter((call) => call[0] === "agent_send_staged")).toHaveLength(1);
    expect(
      screen.queryByRole("button", { name: messages.error.actionRetry }),
    ).not.toBeInTheDocument();

    // The refresh failure is still surfaced, but as a distinct non-resend notice.
    expect(screen.getByText(messages.error.refreshAfterSend.title)).toBeInTheDocument();
    expect(screen.getByText(messages.error.refreshAfterSend.message)).toBeInTheDocument();
  });

  it("cancels an in-flight task and refreshes", async () => {
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} agentPhase="working" />);

    await userEvent.click(screen.getByRole("button", { name: messages.common.cancel }));

    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith("agent_cancel", { projectId }));
    expect(baseProps.onRefresh).toHaveBeenCalled();
  });

  it("never renders durable but unattached materials as conversation history", () => {
    const project = makeProject();
    render(<WorkspaceView project={project} {...baseProps} />);
    expect(screen.queryByRole("button", { name: `Abrir ${materials[1].displayName}` })).toBeNull();
    expect(
      screen.queryAllByRole("button", { name: `Abrir ${materials[0].displayName}` }).length,
    ).toBe(1);
    expect(screen.queryByText(messages.timeline.unattachedTitle)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: messages.material.addFile }),
    ).not.toBeInTheDocument();
  });

  it("shows a lightweight drop overlay only while dragging over the conversation", async () => {
    const { dropHandler } = mockDragDrop();
    setupApi();
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    expect(screen.queryByText(messages.material.dropOverlay)).not.toBeInTheDocument();
    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    act(() => {
      dropHandler({ payload: { type: "over" } });
    });
    expect(await screen.findByText(messages.material.dropOverlay)).toBeInTheDocument();
    act(() => {
      dropHandler({ payload: { type: "leave" } });
    });
    await waitFor(() =>
      expect(screen.queryByText(messages.material.dropOverlay)).not.toBeInTheDocument(),
    );
  });

  it("stages dropped files only in the composer until Send accepts the turn", async () => {
    const stagedReport: StagedAttachmentsReport = {
      items: [{ sourceName: "rosco-data.txt", status: "ready" }],
    };
    const { dropHandler } = mockDragDrop();
    setupApi({ stagedReport });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());

    act(() => {
      dropHandler({ payload: { type: "drop", paths: ["/fake/rosco-data.txt"] } });
    });

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("attachments_stage_paths", {
        paths: ["/fake/rosco-data.txt"],
      }),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "materials_add_from_paths")).toBe(
      false,
    );
    expect(baseProps.onRefresh).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Quitar rosco-data.txt" })).toBeInTheDocument();
    expect(screen.queryByText("rosco-data.txt")?.closest(".chat-log")).toBeNull();

    const textarea = screen.getByLabelText("Pedido a la IA");
    await userEvent.type(textarea, "Usá estos datos para el rosco.");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send_staged", {
        projectId,
        prompt: "Usá estos datos para el rosco.",
        stagedPaths: ["/fake/rosco-data.txt"],
      }),
    );
  });

  for (const count of [1, 5, 8, 9, 16, 51, 100]) {
    it(`keeps the composer reachable and collapses ${count} dropped-file results by default`, async () => {
      const batch: StagedAttachmentsReport = {
        items: Array.from({ length: count }, (_, index) => ({
          sourceName: `nota-${index + 1}.md`,
          status: "ready" as const,
        })),
      };
      const { dropHandler } = mockDragDrop();
      setupApi({ stagedReport: batch });
      render(<WorkspaceView project={makeProject()} {...baseProps} />);

      await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
      act(() => {
        dropHandler({
          payload: { type: "drop", paths: batch.items.map((item) => `/fake/${item.sourceName}`) },
        });
      });

      await waitFor(() =>
        expect(
          screen.getByText(
            count <= 5 ? "nota-1.md" : `${count} archivos seleccionados · Ver todos`,
          ),
        ).toBeInTheDocument(),
      );
      const composer = screen.getByLabelText("Pedido a la IA").closest(".workspace-composer");
      expect(composer).not.toBeNull();
      expect(within(composer as HTMLElement).getByRole("button", { name: "Enviar" })).toBeVisible();

      if (count > 5) {
        expect(screen.queryByText(`nota-${count}.md`)).not.toBeInTheDocument();
      }
    });
  }

  it("keeps duplicate selection state local and visible after expanding", async () => {
    const stagedReport: StagedAttachmentsReport = {
      items: [
        { sourceName: "nueva.md", status: "ready" },
        { sourceName: "previa.md", status: "duplicate_in_selection" },
      ],
    };
    const { dropHandler } = mockDragDrop();
    setupApi({ stagedReport });
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    act(() => {
      dropHandler({ payload: { type: "drop", paths: ["/fake/nueva.md", "/fake/previa.md"] } });
    });
    await waitFor(() =>
      expect(screen.getByText("previa.md · repetido en esta selección")).toBeVisible(),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "materials_add_from_paths")).toBe(
      false,
    );
  });

  it("never persists dropped files while a turn is working", async () => {
    const stagedReport: StagedAttachmentsReport = {
      items: [{ sourceName: "rosco-data.txt", status: "ready" }],
    };
    const { dropHandler } = mockDragDrop();
    setupApi({ stagedReport });
    render(<WorkspaceView project={makeProject()} {...baseProps} agentPhase="working" />);

    act(() => {
      dropHandler({ payload: { type: "drop", paths: ["/fake/rosco-data.txt"] } });
    });

    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("attachments_stage_paths", {
        paths: ["/fake/rosco-data.txt"],
      }),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "materials_add_from_paths")).toBe(
      false,
    );
  });

  it("shows the creating status while the agent is working", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} agentPhase="working" />);
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
  });

  it("renders Compartir controls in the composer bar and on each creation card", () => {
    render(<WorkspaceView project={makeProject()} {...baseProps} />);
    const composerShare = screen.getByRole("button", { name: messages.sharing.shareAction });
    const cardShare = screen.getByRole("button", {
      name: `${messages.sharing.shareAction}: ${creations[0].displayName}`,
    });
    expect(composerShare).toBeInTheDocument();
    expect(cardShare).toBeInTheDocument();
  });

  it("treats an ai_unavailable send error as transient while the backend is starting", async () => {
    setupApi({
      agentSendError: {
        code: "ai_unavailable",
        message: "No se pudo iniciar el asistente de IA.",
      },
    });
    render(<WorkspaceView project={makeProject()} {...baseProps} backendStatus="starting" />);

    await waitFor(() => expect(screen.getByLabelText("Pedido a la IA")).toBeEnabled());
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "Hola");
    await userEvent.click(screen.getByRole("button", { name: messages.common.send }));

    await waitFor(() => expect(screen.getByText(messages.assistant.starting)).toBeInTheDocument());
    expect(screen.queryByText(messages.error.aiUnavailable.title)).not.toBeInTheDocument();
    expect(screen.queryByText("Hola", { selector: ".chat-log" })).toBeNull();
  });

  it("shows a terminal ai_unavailable error when the backend has failed", () => {
    render(
      <WorkspaceView
        project={makeProject()}
        {...baseProps}
        backendStatus="failed"
        aiUsable={false}
      />,
    );
    expect(screen.getByText(messages.error.aiUnavailable.title)).toBeInTheDocument();
    expect(screen.queryByText(messages.assistant.starting)).not.toBeInTheDocument();
  });

  it("reconstructs the processing card from the durable operation read model", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_lexical",
          agentState: "not_started",
          total: 52,
          copied: 52,
          lexicalCompleted: 0,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          materialsReady: 0,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 0 de 52 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.queryByText("52 archivos listos")).not.toBeInTheDocument();
    expect(screen.queryByText(messages.error.storageUnavailable.title)).not.toBeInTheDocument();
  });

  it("shows incremental embedding progress in the compact line and the details popover", async () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 12540,
          failed: 0,
          embeddingsCreated: 12540,
          embeddingsReused: 0,
          chunksTotal: 31778,
          embeddingsTotal: 31778,
          materialsReady: 17,
          elapsedMs: 451000,
          throughputEmbeddingsPerSec: 28.4,
        })}
        {...baseProps}
      />,
    );
    // Compact primary line stays compact and truthful.
    expect(
      screen.getByText("Procesando tu solicitud · 39% · 17 de 50 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.queryByText("Preparación: 50 / 50")).not.toBeInTheDocument();
    // The detailed counters move to the info popover.
    await userEvent.click(screen.getByRole("button", { name: messages.progressDetails.infoAria }));
    expect(screen.getByText("Embeddings completados")).toBeInTheDocument();
    expect(screen.getByText("12.540 / 31.778")).toBeInTheDocument();
    expect(screen.getByText("Embeddings creados")).toBeInTheDocument();
    expect(screen.getByText("12.540")).toBeInTheDocument();
    expect(screen.getByText("~28 /s")).toBeInTheDocument();
    expect(screen.getByText("7m 31s")).toBeInTheDocument();
  });

  it("falls back to the processing line before embedding totals are known", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_lexical",
          agentState: "not_started",
          total: 52,
          copied: 52,
          lexicalCompleted: 12,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 0 de 52 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/Embeddings:/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Fragmentación:/)).not.toBeInTheDocument();
  });

  it("never fabricates percentages or a misleading ETA", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 100,
          failed: 0,
          embeddingsCreated: 100,
          embeddingsReused: 0,
          chunksTotal: 200,
          embeddingsTotal: 200,
          elapsedMs: 60000,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 50% · 0 de 50 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/ETA/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/restan/i)).not.toBeInTheDocument();
  });

  it("shows a truthful synthesis phase instead of 99% while summaries generate", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "started_outcome_unknown",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 31778,
          failed: 0,
          embeddingsCreated: 31778,
          embeddingsReused: 0,
          chunksTotal: 31778,
          embeddingsTotal: 31778,
          materialsReady: 50,
          synthesizing: true,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("Generando resúmenes de 50 archivos…")).toBeInTheDocument();
    expect(screen.queryByText(/Procesando tu solicitud · 99%/)).not.toBeInTheDocument();
  });

  it("re-renders the compact progress without resetting previous counts", async () => {
    const { rerender } = render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 1,
          copied: 1,
          lexicalCompleted: 1,
          embeddingCompleted: 1000,
          failed: 0,
          embeddingsCreated: 1000,
          embeddingsReused: 0,
          chunksTotal: 2000,
          embeddingsTotal: 2000,
          materialsReady: 0,
          elapsedMs: 45000,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 50% · 0 de 1 archivo listo"),
    ).toBeInTheDocument();
    rerender(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 1,
          copied: 1,
          lexicalCompleted: 1,
          embeddingCompleted: 1500,
          failed: 0,
          embeddingsCreated: 1500,
          embeddingsReused: 0,
          chunksTotal: 2000,
          embeddingsTotal: 2000,
          materialsReady: 1,
          elapsedMs: 70000,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 75% · 1 de 1 archivo listo"),
    ).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: messages.progressDetails.infoAria }));
    expect(screen.getByText("1.500 / 2.000")).toBeInTheDocument();
    expect(screen.getByText("Embeddings creados")).toBeInTheDocument();
    expect(screen.getByText("1.500")).toBeInTheDocument();
  });

  it("shows a compact completed state instead of Procesando once the operation is terminal (CASE B)", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 1,
          copied: 1,
          lexicalCompleted: 1,
          embeddingCompleted: 1,
          failed: 0,
          embeddingsCreated: 1,
          embeddingsReused: 0,
          chunksTotal: 1,
          embeddingsTotal: 1,
          materialsReady: 1,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("1 archivo listo")).toBeInTheDocument();
    expect(screen.queryByText("0 de 1 archivo listo")).not.toBeInTheDocument();
    expect(screen.queryByText("Procesando 1 archivos…")).not.toBeInTheDocument();
    expect(screen.queryByText(messages.processing.semanticUnavailable)).not.toBeInTheDocument();
  });

  it("shows a truthful degraded terminal state when semantic indexing failed (CASE A)", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 1,
          copied: 1,
          lexicalCompleted: 1,
          embeddingCompleted: 0,
          failed: 1,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          materialsReady: 0,
        })}
        {...baseProps}
      />,
    );
    expect(screen.queryByText("1 de 1 archivo listo")).not.toBeInTheDocument();
    expect(screen.queryByText(/1 de 1 archivo listo/)).not.toBeInTheDocument();
    expect(screen.getByText("0 de 1 archivo listo · 1 con error")).toBeInTheDocument();
    expect(screen.getByText("0 de 1 archivo listo", { exact: false })).toBeInTheDocument();
    expect(screen.getByText("1 con error", { exact: false })).toBeInTheDocument();
    expect(screen.getByText(messages.processing.semanticUnavailable)).toBeInTheDocument();
    expect(screen.queryByText("Procesando 1 archivos…")).not.toBeInTheDocument();
  });

  it("shows a degraded terminal state when chunks exist but embeddings are incomplete", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 1000,
          failed: 1,
          embeddingsCreated: 1000,
          embeddingsReused: 0,
          chunksTotal: 2000,
          embeddingsTotal: 2000,
          materialsReady: 49,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("49 de 50 archivos listos · 1 con error")).toBeInTheDocument();
    expect(screen.getByText(messages.processing.semanticUnavailable)).toBeInTheDocument();
    expect(screen.queryByText("50 de 50 archivos listos")).not.toBeInTheDocument();
  });

  it("shows a clean completed terminal when every required chunk was reused", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 3,
          copied: 3,
          lexicalCompleted: 3,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 2000,
          chunksTotal: 2000,
          embeddingsTotal: 0,
          materialsReady: 3,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("3 archivos listos")).toBeInTheDocument();
    expect(screen.queryByText(messages.processing.semanticUnavailable)).not.toBeInTheDocument();
    expect(screen.queryByText(/con problemas/)).not.toBeInTheDocument();
  });

  it("shows typed recoverable copy for a pending_retry local interruption", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "pending_retry",
          agentState: "not_started",
          total: 52,
          copied: 52,
          lexicalCompleted: 0,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText(messages.processing.pendingRetry)).toBeInTheDocument();
    const retry = screen.getByRole("button", { name: messages.common.retry });
    expect(retry).toBeInTheDocument();
  });

  it("never shows generic fatal copy for an outcome-unknown remote state", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "pending_retry",
          agentState: "started_outcome_unknown",
          total: 52,
          copied: 52,
          lexicalCompleted: 0,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText(messages.processing.outcomeUnknown)).toBeInTheDocument();
    expect(screen.queryByText(messages.error.internal.title)).not.toBeInTheDocument();
    expect(screen.queryByText(messages.error.storageUnavailable.title)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: messages.common.retry })).not.toBeInTheDocument();
  });

  it("invokes the explicit resume command when Reintentar is pressed", async () => {
    setupApi();
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-52",
          state: "pending_retry",
          agentState: "not_started",
          total: 52,
          copied: 52,
          lexicalCompleted: 0,
          embeddingCompleted: 0,
          failed: 1,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: messages.common.retry }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_resume_import", {
        projectId,
        operationId: "op-52",
      }),
    );
    expect(baseProps.onRefresh).toHaveBeenCalled();
    expect(invokeMock.mock.calls.some((call) => call[0] === "agent_send_staged")).toBe(false);
  });

  it("invokes the explicit summary retry command when Reintentar is pressed for RetryRequired", async () => {
    setupApi();
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-52",
          state: "indexing_embeddings",
          agentState: "started_outcome_unknown",
          summaryRetryable: true,
          total: 52,
          copied: 52,
          lexicalCompleted: 52,
          embeddingCompleted: 52,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: messages.common.retry }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_retry_summary", {
        projectId,
        operationId: "op-52",
      }),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "agent_resume_import")).toBe(false);
  });

  it("shows typed recoverable copy after an auto-resume failure and retries through the same path", async () => {
    const onResumeRetry = vi.fn();
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-52",
          state: "copying",
          agentState: "not_started",
          total: 52,
          copied: 52,
          lexicalCompleted: 0,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
        })}
        {...baseProps}
        resumeFailure="op-52"
        onResumeRetry={onResumeRetry}
      />,
    );
    expect(screen.getByText(messages.processing.resumeFailed)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: messages.common.retry }));
    expect(onResumeRetry).toHaveBeenCalledWith("op-52");
  });

  it("uses grammatically correct singular wording for a single file (CASE 10)", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 1,
          copied: 1,
          lexicalCompleted: 1,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          materialsReady: 0,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("Procesando tu solicitud · 0 de 1 archivo listo")).toBeInTheDocument();
    expect(screen.queryByText("Procesando 1 archivos…")).not.toBeInTheDocument();
  });

  it("shows exactly X de N and never claims N/N before every file is usable (CASE 2 / CASE C)", () => {
    const { rerender } = render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 10000,
          failed: 0,
          embeddingsCreated: 10000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 49,
        })}
        {...baseProps}
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 50% · 49 de 50 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.queryByText("50 de 50 archivos listos")).not.toBeInTheDocument();
    // The huge final file finishes: only then 50/50 appears.
    rerender(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 20000,
          failed: 0,
          embeddingsCreated: 20000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 50,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("50 archivos listos")).toBeInTheDocument();
  });

  it("keeps the compact layout stable while the detail values update live (CASE 8)", async () => {
    const { rerender } = render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 5000,
          failed: 0,
          embeddingsCreated: 5000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 10,
          elapsedMs: 30000,
        })}
        {...baseProps}
      />,
    );
    const infoButton = screen.getByRole("button", {
      name: messages.progressDetails.infoAria,
    });
    await userEvent.click(infoButton);
    expect(screen.getByText("Archivos listos")).toBeInTheDocument();
    expect(screen.getByText("10 de 50")).toBeInTheDocument();
    expect(screen.getByText("5.000 / 20.000")).toBeInTheDocument();
    // The compact primary line must remain exactly the same element/text while
    // the popover content changes underneath it.
    const primaryBefore = screen.getByText(
      "Procesando tu solicitud · 25% · 10 de 50 archivos listos",
    );
    rerender(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 8000,
          failed: 0,
          embeddingsCreated: 8000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 12,
          elapsedMs: 45000,
        })}
        {...baseProps}
      />,
    );
    expect(primaryBefore).toBeInTheDocument();
    expect(
      screen.getByText("Procesando tu solicitud · 40% · 12 de 50 archivos listos"),
    ).toBeInTheDocument();
    expect(screen.getByText("12 de 50")).toBeInTheDocument();
    expect(screen.getByText("8.000 / 20.000")).toBeInTheDocument();
    expect(screen.getByText("45s")).toBeInTheDocument();
  });

  it("reveals details on keyboard focus and closes with Escape (CASE 9)", async () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          chunksTotal: 100,
          embeddingsTotal: 100,
          materialsReady: 3,
        })}
        {...baseProps}
      />,
    );
    const infoButton = screen.getByRole("button", {
      name: messages.progressDetails.infoAria,
    });
    expect(screen.queryByRole("region", { name: messages.progressDetails.infoAria })).toBeNull();
    // Keyboard focus reveals the panel and marks it expanded.
    fireEvent.focus(infoButton);
    expect(
      screen.getByRole("region", { name: messages.progressDetails.infoAria }),
    ).toBeInTheDocument();
    expect(infoButton).toHaveAttribute("aria-expanded", "true");
    expect(infoButton).toHaveAttribute("aria-controls", "import-progress-details");
    // Escape closes it again.
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByRole("region", { name: messages.progressDetails.infoAria })).toBeNull();
    expect(infoButton).toHaveAttribute("aria-expanded", "false");
  });

  it("never shows N/N until materialsReady equals the accepted total (CASE 12)", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "completed",
          agentState: "completed",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 0,
        })}
        {...baseProps}
      />,
    );
    expect(screen.getByText("0 de 50 archivos listos")).toBeInTheDocument();
    expect(screen.queryByText("50 archivos listos")).not.toBeInTheDocument();
    expect(screen.getByText(messages.processing.semanticUnavailable)).toBeInTheDocument();
  });

  it("does not render import progress or details for a zero-material turn", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "ordinary-turn-op",
          state: "completed",
          agentState: "completed",
          total: 0,
          materialsReady: 0,
        })}
        {...baseProps}
        agentPhase="working"
      />,
    );
    expect(screen.queryByText("0 de 0 archivos listos")).toBeNull();
    expect(screen.queryByText("0 archivos listos")).toBeNull();
    expect(screen.queryByRole("button", { name: messages.progressDetails.infoAria })).toBeNull();
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
  });

  it("shows the merged single status line instead of two independent lines", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 10000,
          failed: 0,
          embeddingsCreated: 10000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 17,
        })}
        {...baseProps}
        agentPhase="working"
      />,
    );
    // One merged compact line, prefixed with the request-in-progress copy.
    expect(
      screen.getByText("Procesando tu solicitud · 50% · 17 de 50 archivos listos"),
    ).toBeInTheDocument();
    // The standalone ChatPanel working line must be suppressed while an import
    // is active so the UI never shows two "Procesando…" lines at once.
    expect(screen.queryByText(messages.agent.creating)).not.toBeInTheDocument();
  });

  it("shows a merged in-flight failure suffix when a file errored mid-processing", () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 10000,
          failed: 1,
          embeddingsCreated: 10000,
          embeddingsReused: 0,
          chunksTotal: 20000,
          embeddingsTotal: 20000,
          materialsReady: 49,
        })}
        {...baseProps}
        agentPhase="working"
      />,
    );
    expect(
      screen.getByText("Procesando tu solicitud · 50% · 49 de 50 archivos listos · 1 con error"),
    ).toBeInTheDocument();
    expect(screen.queryByText(messages.agent.creating)).not.toBeInTheDocument();
  });

  it("renders the progress popover through a portal into document.body", async () => {
    const { container } = render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          chunksTotal: 100,
          embeddingsTotal: 100,
          materialsReady: 3,
        })}
        {...baseProps}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: messages.progressDetails.infoAria }));
    const panel = screen.getByRole("region", { name: messages.progressDetails.infoAria });
    // Portal: the panel lives directly under document.body, never inside the
    // scrolling timeline container that would clip it.
    expect(panel.parentElement).toBe(document.body);
    expect(container.querySelector(".workspace-timeline")?.contains(panel)).toBe(false);
    expect(document.body.contains(panel)).toBe(true);
  });

  it("positions the popover within the viewport and clamps it away from the right edge", async () => {
    const originalInnerWidth = window.innerWidth;
    const originalInnerHeight = window.innerHeight;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 400 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 300 });
    const proto = HTMLElement.prototype;
    const widthDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetWidth");
    const heightDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetHeight");
    Object.defineProperty(proto, "offsetWidth", { configurable: true, get: () => 280 });
    Object.defineProperty(proto, "offsetHeight", { configurable: true, get: () => 150 });
    try {
      render(
        <WorkspaceView
          project={makeAcceptedImport({
            operationId: "op-1",
            state: "indexing_embeddings",
            agentState: "not_started",
            total: 50,
            copied: 50,
            lexicalCompleted: 50,
            embeddingCompleted: 0,
            failed: 0,
            embeddingsCreated: 0,
            embeddingsReused: 0,
            chunksTotal: 100,
            embeddingsTotal: 100,
            materialsReady: 3,
          })}
          {...baseProps}
        />,
      );
      const infoButton = screen.getByRole("button", { name: messages.progressDetails.infoAria });
      // Button near the right edge of the 400px viewport.
      infoButton.getBoundingClientRect = () =>
        ({
          top: 100,
          left: 380,
          right: 396,
          bottom: 116,
          width: 16,
          height: 16,
          x: 380,
          y: 100,
          toJSON: () => ({}),
        }) as DOMRect;
      await userEvent.click(infoButton);
      const panel = screen.getByRole("region", { name: messages.progressDetails.infoAria });
      // Clamped: right edge of the 280px panel stays inside the 400px viewport.
      expect(panel.style.left).toBe("112px");
      expect(panel.style.top).toBe("124px");
      expect(panel.style.position).toBe("fixed");
    } finally {
      if (widthDescriptor) Object.defineProperty(proto, "offsetWidth", widthDescriptor);
      else delete (proto as { offsetWidth?: unknown }).offsetWidth;
      if (heightDescriptor) Object.defineProperty(proto, "offsetHeight", heightDescriptor);
      else delete (proto as { offsetHeight?: unknown }).offsetHeight;
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalInnerWidth,
      });
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalInnerHeight,
      });
    }
  });

  it("flips the popover above the button when there is no room below", async () => {
    const originalInnerWidth = window.innerWidth;
    const originalInnerHeight = window.innerHeight;
    Object.defineProperty(window, "innerWidth", { configurable: true, value: 400 });
    Object.defineProperty(window, "innerHeight", { configurable: true, value: 300 });
    const proto = HTMLElement.prototype;
    const widthDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetWidth");
    const heightDescriptor = Object.getOwnPropertyDescriptor(proto, "offsetHeight");
    Object.defineProperty(proto, "offsetWidth", { configurable: true, get: () => 280 });
    Object.defineProperty(proto, "offsetHeight", { configurable: true, get: () => 150 });
    try {
      render(
        <WorkspaceView
          project={makeAcceptedImport({
            operationId: "op-1",
            state: "indexing_embeddings",
            agentState: "not_started",
            total: 50,
            copied: 50,
            lexicalCompleted: 50,
            embeddingCompleted: 0,
            failed: 0,
            embeddingsCreated: 0,
            embeddingsReused: 0,
            chunksTotal: 100,
            embeddingsTotal: 100,
            materialsReady: 3,
          })}
          {...baseProps}
        />,
      );
      const infoButton = screen.getByRole("button", { name: messages.progressDetails.infoAria });
      // Button near the bottom edge of the 300px viewport: no room below.
      infoButton.getBoundingClientRect = () =>
        ({
          top: 280,
          left: 20,
          right: 36,
          bottom: 296,
          width: 16,
          height: 16,
          x: 20,
          y: 280,
          toJSON: () => ({}),
        }) as DOMRect;
      await userEvent.click(infoButton);
      const panel = screen.getByRole("region", { name: messages.progressDetails.infoAria });
      // Flipped above the button (top < button top).
      expect(Number.parseFloat(panel.style.top)).toBeLessThan(280);
      expect(panel.style.top).toBe("122px");
    } finally {
      if (widthDescriptor) Object.defineProperty(proto, "offsetWidth", widthDescriptor);
      else delete (proto as { offsetWidth?: unknown }).offsetWidth;
      if (heightDescriptor) Object.defineProperty(proto, "offsetHeight", heightDescriptor);
      else delete (proto as { offsetHeight?: unknown }).offsetHeight;
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        value: originalInnerWidth,
      });
      Object.defineProperty(window, "innerHeight", {
        configurable: true,
        value: originalInnerHeight,
      });
    }
  });

  it("toggles the popover closed when the info button is pressed again", async () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          chunksTotal: 100,
          embeddingsTotal: 100,
          materialsReady: 3,
        })}
        {...baseProps}
      />,
    );
    const infoButton = screen.getByRole("button", { name: messages.progressDetails.infoAria });
    await userEvent.click(infoButton);
    expect(
      screen.getByRole("region", { name: messages.progressDetails.infoAria }),
    ).toBeInTheDocument();
    await userEvent.click(infoButton);
    expect(screen.queryByRole("region", { name: messages.progressDetails.infoAria })).toBeNull();
    expect(infoButton).toHaveAttribute("aria-expanded", "false");
  });

  it("closes the popover on an outside click", async () => {
    render(
      <WorkspaceView
        project={makeAcceptedImport({
          operationId: "op-1",
          state: "indexing_embeddings",
          agentState: "not_started",
          total: 50,
          copied: 50,
          lexicalCompleted: 50,
          embeddingCompleted: 0,
          failed: 0,
          embeddingsCreated: 0,
          embeddingsReused: 0,
          chunksTotal: 100,
          embeddingsTotal: 100,
          materialsReady: 3,
        })}
        {...baseProps}
      />,
    );
    const infoButton = screen.getByRole("button", { name: messages.progressDetails.infoAria });
    await userEvent.click(infoButton);
    expect(
      screen.getByRole("region", { name: messages.progressDetails.infoAria }),
    ).toBeInTheDocument();
    await userEvent.click(document.body);
    expect(screen.queryByRole("region", { name: messages.progressDetails.infoAria })).toBeNull();
  });

  describe("scroll behavior", () => {
    const scrollState = { scrollHeight: 1000, clientHeight: 100 };

    function installScrollMetrics() {
      Object.defineProperty(HTMLElement.prototype, "scrollHeight", {
        configurable: true,
        get: () => scrollState.scrollHeight,
      });
      Object.defineProperty(HTMLElement.prototype, "clientHeight", {
        configurable: true,
        get: () => scrollState.clientHeight,
      });
    }

    afterEach(() => {
      delete (HTMLElement.prototype as { scrollHeight?: unknown }).scrollHeight;
      delete (HTMLElement.prototype as { clientHeight?: unknown }).clientHeight;
      scrollState.scrollHeight = 1000;
      scrollState.clientHeight = 100;
    });

    function manyMessages(count: number): MessageView[] {
      return Array.from({ length: count }, (_, index) => ({
        id: `scroll-msg-${index}`,
        role: (index % 2 === 0 ? "user" : "assistant") as MessageView["role"],
        text: `mensaje de prueba ${index}`,
        status: "ok" as const,
        createdAt: `2026-08-28T15:${String(index % 60).padStart(2, "0")}:00Z`,
        materialIds: [],
        creationIds: [],
      }));
    }

    it("positions a freshly opened conversation at its latest message", async () => {
      installScrollMetrics();
      const project = makeProject();
      project.messages = manyMessages(30);
      const { container } = render(<WorkspaceView project={project} {...baseProps} />);
      const timeline = container.querySelector(".workspace-timeline") as HTMLElement;
      await waitFor(() => expect(timeline.scrollTop).toBe(scrollState.scrollHeight));
    });

    it("keeps following live content while the user is near the bottom", async () => {
      installScrollMetrics();
      const project = makeProject();
      project.messages = manyMessages(10);
      const { container, rerender } = render(<WorkspaceView project={project} {...baseProps} />);
      const timeline = container.querySelector(".workspace-timeline") as HTMLElement;
      await waitFor(() => expect(timeline.scrollTop).toBe(scrollState.scrollHeight));

      // Simulate the user near the bottom of the current content.
      timeline.scrollTop = 950;
      fireEvent.scroll(timeline);

      scrollState.scrollHeight = 1500;
      const grown = makeProject();
      grown.messages = manyMessages(20);
      rerender(<WorkspaceView project={grown} {...baseProps} />);

      await waitFor(() => expect(timeline.scrollTop).toBe(scrollState.scrollHeight));
    });

    it("does not yank the user back down after scrolling upward", async () => {
      installScrollMetrics();
      const project = makeProject();
      project.messages = manyMessages(10);
      const { container, rerender } = render(<WorkspaceView project={project} {...baseProps} />);
      const timeline = container.querySelector(".workspace-timeline") as HTMLElement;
      await waitFor(() => expect(timeline.scrollTop).toBe(scrollState.scrollHeight));

      // User scrolls up to read older content; far from the bottom.
      timeline.scrollTop = 200;
      fireEvent.scroll(timeline);

      scrollState.scrollHeight = 1500;
      const grown = makeProject();
      grown.messages = manyMessages(20);
      rerender(<WorkspaceView project={grown} {...baseProps} />);

      // Let any spurious follow-up rAF fire, then assert the position held.
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 40));
      });
      expect(timeline.scrollTop).toBe(200);
    });
  });
});
