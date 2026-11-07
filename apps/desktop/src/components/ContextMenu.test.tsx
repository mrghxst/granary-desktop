import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ContextMenu } from "./ContextMenu";

const actions = [
  { label: "Play", onSelect: () => undefined },
  { label: "Details", onSelect: () => undefined },
];

describe("ContextMenu", () => {
  it("keeps focus on the chosen item when the parent re-renders with a new onClose", () => {
    const { rerender } = render(<ContextMenu x={10} y={10} actions={actions} label="Menu" onClose={() => undefined} />);
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Play" }));
    fireEvent.keyDown(screen.getByRole("menu"), { key: "ArrowDown" });
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Details" }));
    // A parent render passes a fresh inline closure.
    rerender(<ContextMenu x={10} y={10} actions={actions} label="Menu" onClose={() => undefined} />);
    expect(document.activeElement).toBe(screen.getByRole("menuitem", { name: "Details" }));
  });

  it("calls the latest onClose on Escape", () => {
    const first = vi.fn();
    const latest = vi.fn();
    const { rerender } = render(<ContextMenu x={10} y={10} actions={actions} label="Menu" onClose={first} />);
    rerender(<ContextMenu x={10} y={10} actions={actions} label="Menu" onClose={latest} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(latest).toHaveBeenCalledTimes(1);
    expect(first).not.toHaveBeenCalled();
  });
});
