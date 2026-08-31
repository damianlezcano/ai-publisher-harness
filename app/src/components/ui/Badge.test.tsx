import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import Badge from "./Badge";

describe("Badge", () => {
  it.each(["ok", "neutral", "warning", "danger"] as const)("renders the %s tone", (tone) => {
    render(<Badge tone={tone}>Etiqueta</Badge>);
    expect(screen.getByText("Etiqueta")).toHaveClass(`badge ${tone}`);
  });

  it("defaults to the neutral tone", () => {
    render(<Badge>Privado</Badge>);
    expect(screen.getByText("Privado")).toHaveClass("badge neutral");
  });
});
