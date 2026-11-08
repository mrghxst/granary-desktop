import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import "../i18n";
import { FilterChips } from "./Library";

const studios = Array.from({ length: 300 }, (_, i) => `Studio ${String(i).padStart(3, "0")}`).concat(["Ghibli"]);

describe("FilterChips", () => {
  it("lists short facets in full without a search field", () => {
    render(<FilterChips title="Genre" values={["Drama", "Comedy"]} value={null} onChange={vi.fn()} />);
    expect(screen.queryByRole("searchbox")).toBeNull();
    expect(screen.getAllByRole("button")).toHaveLength(2);
  });

  it("limits long facets, searches them and keeps the selected value visible", () => {
    const onChange = vi.fn();
    render(<FilterChips title="Studio" values={studios} value="Studio 250" onChange={onChange} />);
    const chips = () => screen.getAllByRole("button").map((b) => b.textContent);
    expect(chips()).toHaveLength(14);
    expect(chips()[0]).toBe("Studio 250");
    expect(screen.getByText(/more, type to narrow down/)).toBeTruthy();
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "ghi" } });
    expect(chips()).toEqual(["Studio 250", "Ghibli"]);
    fireEvent.click(screen.getByRole("button", { name: "Ghibli" }));
    expect(onChange).toHaveBeenCalledWith("Ghibli");
    fireEvent.change(screen.getByRole("searchbox"), { target: { value: "zzz" } });
    expect(chips()).toEqual(["Studio 250"]);
  });
});
