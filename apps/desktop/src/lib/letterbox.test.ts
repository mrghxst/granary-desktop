import { describe, expect, it } from "vitest";
import { pictureBand } from "./letterbox";

describe("pictureBand", () => {
  it("finds the bars of a scope film in a 16:9 window", () => {
    const band = pictureBand(1920, 1080, 2.39);
    expect(band?.top).toBeCloseTo(138.3, 1);
    expect(band?.bottom).toBeCloseTo(941.7, 1);
  });

  it("has nothing to light for a picture that fills the height, or before the aspect is known", () => {
    expect(pictureBand(1920, 1080, 16 / 9)).toBeNull();
    expect(pictureBand(1920, 1080, 4 / 3)).toBeNull();
    expect(pictureBand(1920, 1080, 0)).toBeNull();
  });
});
