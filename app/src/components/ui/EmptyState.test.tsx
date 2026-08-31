import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import EmptyState from "./EmptyState";

describe("EmptyState", () => {
  it("renders title, body, and primary action", () => {
    const onAction = vi.fn();
    render(
      <EmptyState
        title="Todavía no tenés proyectos"
        body="Creá uno para empezar."
        actionLabel="Crear proyecto"
        onAction={onAction}
      />,
    );
    expect(screen.getByText("Todavía no tenés proyectos")).toHaveClass("empty-state-title");
    expect(screen.getByText("Creá uno para empezar.")).toHaveClass("empty-state-body");
    const action = screen.getByRole("button", { name: "Crear proyecto" });
    expect(action).toHaveClass("primary");
  });

  it("renders an optional secondary action", async () => {
    const user = userEvent.setup();
    const onSecondary = vi.fn();
    render(
      <EmptyState
        title="Pedile a la IA que cree algo"
        secondaryLabel="Escribí en el asistente"
        onSecondary={onSecondary}
      />,
    );
    const secondary = screen.getByRole("button", { name: "Escribí en el asistente" });
    expect(secondary).toHaveClass("secondary");
    await user.click(secondary);
    expect(onSecondary).toHaveBeenCalledTimes(1);
  });

  it("renders no action area when no action is provided", () => {
    render(<EmptyState title="Todavía no tenés proyectos" />);
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });
});
