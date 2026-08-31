import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ErrorNotice from "./ErrorNotice";

describe("ErrorNotice", () => {
  it("renders role=alert with guidance title, message, and actions", () => {
    render(
      <ErrorNotice
        guidance={{
          title: "No pudimos compartir en este momento.",
          message: "No pudimos compartir en este momento. comprobá tu conexión a Internet",
          actions: ["retry"],
        }}
      />,
    );
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("No pudimos compartir en este momento.");
    expect(alert).toHaveTextContent("comprobá tu conexión a Internet");
    const action = screen.getByRole("button", { name: "Reintentar" });
    expect(action).toHaveClass("secondary");
  });

  it("derives guidance from an error via its code", () => {
    render(<ErrorNotice error={{ code: "credential_revoked", message: "x" }} />);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveTextContent("Necesitás volver a conectar tu cuenta.");
    expect(screen.getByRole("button", { name: "Conectar IA" })).toHaveClass("primary");
  });

  it("never renders raw codes, paths, or stack traces", () => {
    render(<ErrorNotice error={{ code: "publish_failed", message: "no se pudo" }} />);
    expect(screen.queryByText(/publish_failed/)).not.toBeInTheDocument();
    expect(screen.getByRole("alert")).toHaveTextContent("No pudimos compartir en este momento.");
  });

  it("renders no action buttons when the guidance has none", () => {
    render(
      <ErrorNotice
        guidance={{
          title: "Ese archivo ya está en el proyecto.",
          message: "Ese archivo ya está en el proyecto.",
          actions: [],
        }}
      />,
    );
    expect(screen.getByRole("alert")).toBeInTheDocument();
    expect(screen.queryAllByRole("button")).toHaveLength(0);
  });

  it("renders nothing when there is no error or guidance", () => {
    const { container } = render(<ErrorNotice />);
    expect(container).toBeEmptyDOMElement();
  });

  it("reports the pressed action kind", async () => {
    const user = userEvent.setup();
    const onAction = vi.fn();
    render(
      <ErrorNotice
        guidance={{ title: "T", message: "M", actions: ["retry", "open-with-app"] }}
        onAction={onAction}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abrir con la aplicación" }));
    expect(onAction).toHaveBeenCalledWith("open-with-app");
  });
});
