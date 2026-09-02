import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ChatPanel from "./ChatPanel";
import { messages } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials = [
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

const creations = [
  {
    id: "c1",
    displayName: "actividad",
    kind: "web",
    visibility: "private" as const,
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
    revision: 1,
  },
];

const base = {
  projectId,
  materials,
  creations,
  messages: [] as {
    id: string;
    role: "user" | "assistant";
    text: string;
    status: "ok" | "failed" | "cancelled";
    createdAt: string;
    materialIds: string[];
    creationIds: string[];
  }[],
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onRefresh: vi.fn(),
};

beforeEach(() => {
  invokeMock.mockReset();
  base.messages = [];
  base.onRefresh.mockReset();
});

describe("ChatPanel timeline", () => {
  it("renders the empty hint when there are no messages or unattached materials", () => {
    render(<ChatPanel {...base} materials={[]} />);
    expect(screen.getByText(messages.assistant.emptyHint)).toBeInTheDocument();
  });

  it("renders unattached materials as user-side attachments, not as assistant output", () => {
    render(<ChatPanel {...base} />);
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[1].displayName}` }),
    ).toBeInTheDocument();
    expect(screen.getByText(materials[0].displayName).closest(".message-user")).toBeTruthy();
    expect(screen.getByText(materials[0].displayName).closest(".creation-card")).toBeNull();
    expect(screen.queryByText(messages.timeline.resourceLabel)).not.toBeInTheDocument();
    expect(screen.queryByText(messages.assistant.emptyHint)).not.toBeInTheDocument();
  });

  it("renders a user message with its text and role label", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Creá una actividad",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByText("Creá una actividad")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.userLabel)).toBeInTheDocument();
  });

  it("renders material chips on a user message and opens a material on click", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Usá este material",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: ["m1"],
            creationIds: [],
          },
        ]}
      />,
    );
    const chip = screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` });
    expect(chip).toBeInTheDocument();
    await userEvent.click(chip);
    expect(invokeMock).toHaveBeenCalledWith("material_open", {
      projectId,
      materialId: "m1",
    });
  });

  it("renders an assistant message with inline creation cards", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-2",
            role: "assistant",
            text: "Acá tenés la actividad",
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    expect(screen.getByText("Acá tenés la actividad")).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.assistantLabel)).toBeInTheDocument();
    expect(screen.getByText(creations[0].displayName)).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${messages.common.open}: ${creations[0].displayName}` }),
    ).toBeInTheDocument();
  });

  it("does not render an empty completed assistant bubble labelled only Asistente", () => {
    const onShare = vi.fn();
    render(
      <ChatPanel
        {...base}
        share={{ onShare, shared: false, busy: false }}
        messages={[
          {
            id: "msg-empty",
            role: "assistant",
            text: "   ",
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: [],
          },
          {
            id: "msg-real",
            role: "assistant",
            text: "Listo. Creé el recurso usando el archivo que adjuntaste.",
            status: "ok",
            createdAt: "2026-08-28T15:02:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    const labels = screen.getAllByText(messages.timeline.assistantLabel);
    expect(labels).toHaveLength(1);
    expect(
      screen.getByText("Listo. Creé el recurso usando el archivo que adjuntaste."),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${messages.common.open}: ${creations[0].displayName}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: `${messages.sharing.shareAction}: ${creations[0].displayName}`,
      }),
    ).toBeInTheDocument();
  });

  it("Abrir and Compartir on a creation card target the same registered creation", async () => {
    const onShare = vi.fn();
    invokeMock.mockResolvedValue(undefined);
    render(
      <ChatPanel
        {...base}
        share={{ onShare, shared: false, busy: false }}
        messages={[
          {
            id: "msg-2",
            role: "assistant",
            text: "Listo.",
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    await userEvent.click(
      screen.getByRole("button", { name: `${messages.common.open}: ${creations[0].displayName}` }),
    );
    expect(invokeMock).toHaveBeenCalledWith("preview_open_web", {
      projectId,
      creationId: "c1",
    });
    await userEvent.click(
      screen.getByRole("button", {
        name: `${messages.sharing.shareAction}: ${creations[0].displayName}`,
      }),
    );
    expect(onShare).toHaveBeenCalledWith("c1");
  });

  it("renders a failed assistant message as an alert without creation cards", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-3",
            role: "assistant",
            text: "No se pudo completar.",
            status: "failed",
            createdAt: "2026-08-28T15:02:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("No se pudo completar.");
    expect(screen.queryByText(creations[0].displayName)).not.toBeInTheDocument();
  });

  it("renders a cancelled assistant message as an alert", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-4",
            role: "assistant",
            text: "Cancelado.",
            status: "cancelled",
            createdAt: "2026-08-28T15:03:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("Cancelado.");
  });

  it("shows the working status with a spinner", () => {
    render(<ChatPanel {...base} agentPhase="working" />);
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
    expect(document.querySelector(".spinner")).toHaveAttribute("aria-hidden", "true");
  });

  it("does not render a raw green completed status that duplicates assistant content", () => {
    const assistantText = "Acá tenés la actividad";
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-2",
            role: "assistant",
            text: assistantText,
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
        agentPhase="completed"
        agentMessage={assistantText}
      />,
    );
    // The assistant text renders once, inside the persisted assistant bubble.
    expect(screen.getAllByText(assistantText)).toHaveLength(1);
    // No green raw status line duplicates it.
    expect(
      screen.queryByText(assistantText, { selector: ".chat-status.ok" }),
    ).not.toBeInTheDocument();
    // The creation card still renders.
    expect(screen.getByText(creations[0].displayName)).toBeInTheDocument();
  });

  it("shows a failed status line as an alert", () => {
    render(<ChatPanel {...base} agentPhase="failed" agentMessage="Falló." />);
    expect(screen.getByRole("alert")).toHaveTextContent("Falló.");
  });

  it("renders an attached material only inside the user bubble, not as a standalone resource", () => {
    render(
      <ChatPanel
        {...base}
        materials={[materials[0]]}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Usá este material",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: ["m1"],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
    expect(screen.queryByText(messages.timeline.resourceLabel)).not.toBeInTheDocument();
  });

  it("renders a pending user attachment inside the bubble and not as a standalone resource", () => {
    render(
      <ChatPanel
        {...base}
        materials={[materials[0]]}
        pendingUser={{ text: "Pendiente", materialIds: ["m1"] }}
        messages={[]}
      />,
    );
    expect(screen.getByText("Pendiente")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
    expect(screen.queryByText(messages.timeline.resourceLabel)).not.toBeInTheDocument();
  });

  it("still renders genuinely unattached materials as user-side attachments", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-1",
            role: "user",
            text: "Sin adjuntos",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[1].displayName}` }),
    ).toBeInTheDocument();
    expect(screen.queryByText(messages.timeline.resourceLabel)).not.toBeInTheDocument();
    expect(screen.getByText(materials[0].displayName).closest(".creation-card")).toBeNull();
  });

  it("renders a pending user message until it matches a persisted message", () => {
    const { rerender } = render(
      <ChatPanel
        {...base}
        pendingUser={{ text: "Pendiente", materialIds: ["m1"] }}
        messages={[]}
      />,
    );
    expect(screen.getByText("Pendiente")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `Abrir ${materials[0].displayName}` }),
    ).toBeInTheDocument();

    rerender(
      <ChatPanel
        {...base}
        pendingUser={{ text: "Pendiente", materialIds: ["m1"] }}
        messages={[
          {
            id: "msg-5",
            role: "user",
            text: "Pendiente",
            status: "ok",
            createdAt: "2026-08-28T15:04:00Z",
            materialIds: ["m1"],
            creationIds: [],
          },
        ]}
      />,
    );
    expect(screen.getByText("Pendiente")).toBeInTheDocument();
    // The pending duplicate is suppressed; only the persisted message chip is rendered.
    const chips = screen.getAllByRole("button", { name: `Abrir ${materials[0].displayName}` });
    expect(chips.length).toBe(1);
  });

  it("keeps the working status non-duplicating while an assistant bubble is present", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-2",
            role: "assistant",
            text: "Acá tenés la actividad",
            status: "ok",
            createdAt: "2026-08-28T15:01:00Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
        agentPhase="working"
      />,
    );
    expect(screen.getByText(messages.agent.creating)).toBeInTheDocument();
    expect(document.querySelector(".spinner")).toHaveAttribute("aria-hidden", "true");
    // The working status does not render the assistant content a second time.
    expect(screen.getAllByText("Acá tenés la actividad")).toHaveLength(1);
  });

  it("does not duplicate a persisted failed assistant message as raw error text", () => {
    const failure = "No se pudo iniciar el asistente de IA.";
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-3",
            role: "assistant",
            text: failure,
            status: "failed",
            createdAt: "2026-08-28T15:02:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
        agentPhase="failed"
        agentMessage={failure}
      />,
    );
    expect(screen.getAllByText(failure)).toHaveLength(1);
    expect(screen.queryByText(failure, { selector: ".chat-status.err" })).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent(failure);
  });

  it("still renders a failed status line when an earlier failed bubble is not the newest message", () => {
    const previousFailure = "No se pudo completar la creación.";
    const currentFailure = "No se pudo iniciar el asistente de IA.";
    render(
      <ChatPanel
        {...base}
        messages={[
          {
            id: "msg-old-fail",
            role: "assistant",
            text: previousFailure,
            status: "failed",
            createdAt: "2026-08-28T15:02:00Z",
            materialIds: [],
            creationIds: [],
          },
          {
            id: "msg-new-user",
            role: "user",
            text: "Intentá de nuevo",
            status: "ok",
            createdAt: "2026-08-28T15:03:00Z",
            materialIds: [],
            creationIds: [],
          },
        ]}
        agentPhase="failed"
        agentMessage={currentFailure}
      />,
    );
    expect(screen.getByText(previousFailure)).toBeInTheDocument();
    const errStatus = screen.getByText(currentFailure, { selector: ".chat-status.err" });
    expect(errStatus).toHaveAttribute("role", "alert");
    expect(screen.getAllByText(currentFailure)).toHaveLength(1);
  });

  it("keeps the polite live region on the chat log for accessibility", () => {
    const { container } = render(<ChatPanel {...base} />);
    expect(container.querySelector(".chat-log")).toHaveAttribute("aria-live", "polite");
  });
});
