import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import WorkspaceView from "./WorkspaceView";
import { messages } from "../messages";
import type {
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
    expect(screen.queryByText("Este mensaje falla")).not.toBeInTheDocument();
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
    expect(screen.queryByText("Hola")).toBeNull();
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
});
