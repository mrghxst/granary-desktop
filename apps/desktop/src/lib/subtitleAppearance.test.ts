import { describe, expect, it } from "vitest";
import { effective, parseAppearance, withMember } from "./subtitleAppearance";

describe("subtitle appearance", () => {
  it("reads a sparse stored value over the default", () => {
    const a = parseAppearance('{"fontSize":"xxlarge"}');
    expect(effective(a, "fontSize")).toBe("xxlarge");
    expect(effective(a, "backgroundStyle")).toBe("box");
  });

  it("treats anything but an object as no override", () => {
    expect(parseAppearance("")).toEqual({});
    expect(parseAppearance("[1]")).toEqual({});
    expect(parseAppearance("null")).toEqual({});
  });

  it("changes one member and keeps the rest", () => {
    expect(JSON.parse(withMember({ fontSize: "small" }, "position", "top"))).toEqual({
      fontSize: "small",
      position: "top",
    });
  });
});
