import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import PublishPanel from "./PublishPanel";
import { messages } from "../messages";
import type { PublicationView } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("qrcode", () => ({
  default: { toDataURL: vi.fn().mockResolvedValue("data:image/png;base64,FAKE") },
}));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const projectName = "Fotosíntesis";
const url = "https://fake.trycloudflare.com/fotosintesis-a7k2m9/";

const local: PublicationView = { state: "local", publicUrl: null };
const published: PublicationView = { state: "published", publicUrl: url };

function renderControl(publication: PublicationView, onRefresh = vi.fn()) {
  return render(
    <PublishPanel
      projectId={projectId}
      projectName={projectName}
      publication={publication}
      onRefresh={onRefresh}
    />,
  );
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("ShareControl", () => {
  it("renders a single Compartir button for a local project", () => {
    renderControl(local);
    expect(screen.getByRole("button", { name: messages.sharing.shareAction })).toBeInTheDocument();
  });

  it("publishes and refreshes when Compartir is clicked", async () => {
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    const gate = deferred<PublicationView>();
    invokeMock.mockReturnValueOnce(gate.promise);
    const { rerender } = renderControl(local, onRefresh);

    await user.click(screen.getByRole("button", { name: messages.sharing.shareAction }));
    expect(invokeMock).toHaveBeenCalledWith("publish", { projectId, creationId: null });

    gate.resolve(published);
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());

    rerender(
      <PublishPanel
        projectId={projectId}
        projectName={projectName}
        publication={published}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByRole("button", { name: messages.sharing.shared })).toBeInTheDocument();
  });

  it("shows a busy Compartiendo… state while publishing", async () => {
    const user = userEvent.setup();
    const gate = deferred<PublicationView>();
    invokeMock.mockReturnValueOnce(gate.promise);
    renderControl(local);

    await user.click(screen.getByRole("button", { name: messages.sharing.shareAction }));
    expect(screen.getByRole("button", { name: messages.sharing.sharing })).toBeDisabled();
    gate.resolve(published);
  });

  it("opens a menu with copy, open, QR and stop-sharing when shared", async () => {
    const user = userEvent.setup();
    renderControl(published);

    const trigger = screen.getByRole("button", { name: messages.sharing.shared });
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    await user.click(trigger);
    expect(trigger).toHaveAttribute("aria-expanded", "true");

    const menu = screen.getByRole("menu", { name: messages.sharing.panelLabel });
    expect(within(menu).getByText(url)).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: messages.sharing.copyLink }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: messages.sharing.openLink }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: messages.sharing.showQr }),
    ).toBeInTheDocument();
    expect(
      within(menu).getByRole("menuitem", { name: messages.sharing.stopSharing }),
    ).toBeEnabled();
    expect(within(menu).getByRole("menuitem", { name: messages.sharing.stopSharing })).toHaveClass(
      "danger",
    );
    expect(within(menu).getByText(messages.sharing.temporaryNote)).toBeInTheDocument();
  });

  it("copies the link and swaps the menu item to Copiado", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    renderControl(published);
    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.copyLink }));
    expect(writeText).toHaveBeenCalledWith(url);
    expect(screen.getByRole("menuitem", { name: messages.common.copied })).toBeInTheDocument();
  });

  it("shows a message when copying fails", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });
    renderControl(published);
    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.copyLink }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(messages.sharing.copyLinkFailed),
    );
  });

  it("opens the public URL via the api", async () => {
    const user = userEvent.setup();
    invokeMock.mockResolvedValueOnce(undefined);
    renderControl(published);
    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.openLink }));
    expect(invokeMock).toHaveBeenCalledWith("open_public_url", { projectId });
  });

  it("shows an ErrorNotice when publishing fails", async () => {
    const user = userEvent.setup();
    invokeMock.mockRejectedValueOnce({ code: "publish_failed", message: "x" });
    renderControl(local);
    await user.click(screen.getByRole("button", { name: messages.sharing.shareAction }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(messages.error.publishFailed.title),
    );
  });

  it("opens a light stop-sharing confirm and unpublishes on confirm", async () => {
    const user = userEvent.setup();
    const onRefresh = vi.fn();
    invokeMock.mockResolvedValueOnce(local);
    const { rerender } = renderControl(published, onRefresh);

    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.stopSharing }));

    const dialog = screen.getByRole("dialog");
    expect(within(dialog).getByText(messages.sharing.stopConfirm.title)).toBeInTheDocument();
    expect(within(dialog).getByText(messages.sharing.stopConfirm.message)).toBeInTheDocument();

    await user.click(within(dialog).getByRole("button", { name: messages.common.confirm }));
    expect(invokeMock).toHaveBeenCalledWith("unpublish", { projectId });
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());

    rerender(
      <PublishPanel
        projectId={projectId}
        projectName={projectName}
        publication={local}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByRole("button", { name: messages.sharing.shareAction })).toBeInTheDocument();
  });

  it("cancels the stop-sharing confirm without unpublishing", async () => {
    const user = userEvent.setup();
    renderControl(published);
    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.stopSharing }));
    await user.click(screen.getByRole("button", { name: messages.common.cancel }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("unpublish", expect.anything());
  });

  it("opens the QR dialog from the share menu", async () => {
    const user = userEvent.setup();
    renderControl(published);
    await user.click(screen.getByRole("button", { name: messages.sharing.shared }));
    await user.click(screen.getByRole("menuitem", { name: messages.sharing.showQr }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: projectName })).toBeInTheDocument();
  });
});
