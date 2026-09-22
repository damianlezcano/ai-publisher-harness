import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ChatPanel from "./ChatPanel";
import { messages } from "../messages";
import type { MessageView } from "../types";

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
    lineageId: "c1",
    versionNumber: 1,
    isCurrent: true,
    availableVersionIds: ["c1"],
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

  it("does not render unattached durable materials as conversation history", () => {
    render(<ChatPanel {...base} />);
    expect(screen.queryByRole("button", { name: `Abrir ${materials[0].displayName}` })).toBeNull();
    expect(screen.queryByRole("button", { name: `Abrir ${materials[1].displayName}` })).toBeNull();
    expect(screen.getByText(messages.assistant.emptyHint)).toBeInTheDocument();
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

  it("renders sequential turns in list order so an older assistant reply cannot follow a newer user message", () => {
    render(
      <ChatPanel
        {...base}
        materials={[]}
        messages={[
          {
            id: "u1",
            role: "user",
            text: "hola!",
            status: "ok",
            createdAt: "2026-08-28T15:00:00Z",
            materialIds: [],
            creationIds: [],
          },
          {
            id: "a1",
            role: "assistant",
            text: "¡Hola! ¿Cómo estás?",
            status: "ok",
            createdAt: "2026-08-28T15:00:01Z",
            materialIds: [],
            creationIds: [],
          },
          {
            id: "u2",
            role: "user",
            text: "creá una actividad",
            status: "ok",
            createdAt: "2026-08-28T15:00:02Z",
            materialIds: [],
            creationIds: [],
          },
          {
            id: "a2",
            role: "assistant",
            text: "Listo. Creé el recurso.",
            status: "ok",
            createdAt: "2026-08-28T15:00:03Z",
            materialIds: [],
            creationIds: ["c1"],
          },
        ]}
      />,
    );
    const log = screen.getByLabelText(messages.assistant.panelLabel);
    const texts = Array.from(log.querySelectorAll(".message-text")).map((el) => el.textContent);
    expect(texts).toEqual([
      "hola!",
      "¡Hola! ¿Cómo estás?",
      "creá una actividad",
      "Listo. Creé el recurso.",
    ]);
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

  it("suppresses the working status when the import progress line owns it", () => {
    render(<ChatPanel {...base} agentPhase="working" suppressWorkingStatus />);
    expect(screen.queryByText(messages.agent.creating)).not.toBeInTheDocument();
  });

  it("shows a synthesis label for a no-attachment summary turn", () => {
    render(<ChatPanel {...base} agentPhase="working" synthesizingSummaries />);
    expect(
      screen.getByText(messages.compactProgress.generatingSummariesIndeterminate),
    ).toBeInTheDocument();
    expect(screen.queryByText(messages.agent.creating)).not.toBeInTheDocument();
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

  it("does not render proposed attachments as history", () => {
    render(<ChatPanel {...base} materials={[materials[0]]} messages={[]} />);
    expect(screen.queryByText("Pendiente")).toBeNull();
    expect(screen.queryByRole("button", { name: `Abrir ${materials[0].displayName}` })).toBeNull();
  });

  it("keeps unattached durable materials out of both conversation sides", () => {
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
    expect(screen.queryByRole("button", { name: `Abrir ${materials[0].displayName}` })).toBeNull();
    expect(screen.queryByRole("button", { name: `Abrir ${materials[1].displayName}` })).toBeNull();
  });

  it("renders attachment cards only after a persisted user turn", () => {
    const { rerender } = render(<ChatPanel {...base} messages={[]} />);
    expect(screen.queryByText("Pendiente")).toBeNull();
    expect(screen.queryByRole("button", { name: `Abrir ${materials[0].displayName}` })).toBeNull();

    rerender(
      <ChatPanel
        {...base}
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
    // The persisted turn is the only source of a right-side attachment card.
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

describe("ChatPanel per-turn metrics", () => {
  const turn = (inputTokens: number | null, sourceNames: string[] = []) => ({
    provider: "opencode",
    model: "big-pickle",
    inputTokens,
    outputTokens: 200,
    cacheReadTokens: null,
    cacheWriteTokens: null,
    totalTokens: null,
    costUsd: null,
    turnDurationMs: 5000,
    source: "provider_actual",
    remoteCalls: 1,
    materialCount: 2,
    corpusBytes: 1000,
    corpusUtf8Chars: 900,
    corpusEstTokens: 300,
    retrievalCandidateCount: 3,
    selectedEvidenceCount: 1,
    selectedEvidenceBytes: 100,
    selectedEvidenceUtf8Chars: 90,
    evidenceEstTokens: 30,
    contextReductionPct: 90,
    semanticProviderState: "available",
    requestPreparationMs: 5,
    retrievalMode: "normal",
    eligibleMaterials: null,
    materialsInspected: null,
    chunksInspected: null,
    exhaustiveCoverage: "not_requested",
    lexicalHits: null,
    semanticHits: null,
    localMode: null,
    sourceNames,
  });

  function user(overrides: Partial<MessageView>) {
    return {
      id: "u1",
      role: "user" as const,
      text: "pregunta",
      status: "ok" as const,
      createdAt: "2026-08-28T15:00:00Z",
      materialIds: [],
      creationIds: [],
      ...overrides,
    };
  }

  function assistant(overrides: Partial<MessageView>) {
    return {
      id: "a1",
      role: "assistant" as const,
      text: "respuesta",
      status: "ok" as const,
      createdAt: "2026-08-28T15:00:01Z",
      materialIds: [],
      creationIds: [],
      ...overrides,
    };
  }

  it("does not render a Fuentes block in the assistant answer bubble", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          user({ turnMetrics: turn(111, ["file-a.md"]) }),
          assistant({ text: "La respuesta limpia, sin fuentes visibles." }),
        ]}
      />,
    );
    expect(screen.getByText("La respuesta limpia, sin fuentes visibles.")).toBeInTheDocument();
    expect(screen.queryByText(/Fuentes:/)).not.toBeInTheDocument();
  });

  it("binds each assistant response to its own per-turn metrics", () => {
    render(
      <ChatPanel
        {...base}
        messages={[
          user({ id: "u1", turnMetrics: turn(111) }),
          assistant({ id: "a1", text: "Primera." }),
          user({ id: "u2", createdAt: "2026-08-28T15:00:02Z", turnMetrics: turn(222) }),
          assistant({ id: "a2", createdAt: "2026-08-28T15:00:03Z", text: "Segunda." }),
        ]}
      />,
    );
    expect(screen.getByText(/111/)).toBeInTheDocument();
    expect(screen.getByText(/222/)).toBeInTheDocument();
  });
});

function manyMaterials(count: number) {
  return Array.from({ length: count }, (_, index) => ({
    id: `mm${index}`,
    displayName: `archivo-${index}.md`,
    originalFileName: `archivo-${index}.md`,
    kind: "text",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
  }));
}

function userMessageWith(materialIds: string[]) {
  return [
    {
      id: "msg-many",
      role: "user" as const,
      text: "haceme un resumen de estos archivos",
      status: "ok" as const,
      createdAt: "2026-08-28T15:00:00Z",
      materialIds,
      creationIds: [] as string[],
    },
  ];
}

describe("ChatPanel attachment collapse", () => {
  it("keeps a single attachment as a natural per-file chip", () => {
    const mats = manyMaterials(1);
    render(
      <ChatPanel {...base} materials={mats} messages={userMessageWith(mats.map((m) => m.id))} />,
    );
    expect(screen.getByRole("button", { name: `Abrir archivo-0.md` })).toBeInTheDocument();
    expect(screen.queryByText(messages.timeline.attachmentsSummary(1))).toBeNull();
  });

  it("keeps a small attachment set expanded without a summary", () => {
    const mats = manyMaterials(3);
    render(
      <ChatPanel {...base} materials={mats} messages={userMessageWith(mats.map((m) => m.id))} />,
    );
    expect(screen.getByRole("button", { name: `Abrir archivo-0.md` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Abrir archivo-2.md` })).toBeInTheDocument();
    expect(screen.queryByText(messages.timeline.attachmentsSummary(3))).toBeNull();
  });

  it("collapses 50 attachments into a compact summary by default", () => {
    const mats = manyMaterials(50);
    render(
      <ChatPanel {...base} materials={mats} messages={userMessageWith(mats.map((m) => m.id))} />,
    );
    expect(screen.getByText(messages.timeline.attachmentsSummary(50))).toBeInTheDocument();
    expect(screen.getByText(messages.timeline.showAttachments)).toBeInTheDocument();
    // No expanded chips dominate the conversation.
    expect(screen.queryByRole("button", { name: `Abrir archivo-0.md` })).toBeNull();
  });

  it("expands the full list on demand and collapses back", async () => {
    const mats = manyMaterials(50);
    render(
      <ChatPanel {...base} materials={mats} messages={userMessageWith(mats.map((m) => m.id))} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Ver archivos/ }));
    expect(screen.getByRole("button", { name: `Abrir archivo-0.md` })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `Abrir archivo-49.md` })).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: messages.timeline.hideAttachments }));
    expect(screen.queryByRole("button", { name: `Abrir archivo-0.md` })).toBeNull();
    expect(screen.getByText(messages.timeline.attachmentsSummary(50))).toBeInTheDocument();
  });

  it("preserves every attachment's Abrir action after expanding", async () => {
    const mats = manyMaterials(50);
    invokeMock.mockResolvedValue(undefined);
    render(
      <ChatPanel {...base} materials={mats} messages={userMessageWith(mats.map((m) => m.id))} />,
    );
    await userEvent.click(screen.getByRole("button", { name: /Ver archivos/ }));
    await userEvent.click(screen.getByRole("button", { name: `Abrir archivo-7.md` }));
    expect(invokeMock).toHaveBeenCalledWith("material_open", {
      projectId,
      materialId: "mm7",
    });
  });
});
