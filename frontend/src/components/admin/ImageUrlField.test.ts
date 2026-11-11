import { afterEach, describe, expect, it, vi } from "vitest";
import { checkMediaFile, MEDIA_MAX, uploadMedia } from "./ImageUrlField";

afterEach(() => vi.restoreAllMocks());

describe("media upload checks", () => {
  it("accepts png, jpeg, webp and gif under the size cap", () => {
    for (const type of ["image/png", "image/jpeg", "image/webp", "image/gif"]) expect(checkMediaFile({ type, size: 1000 })).toBeNull();
  });
  it("rejects other types and big files", () => {
    expect(checkMediaFile({ type: "image/svg+xml", size: 10 })).toBe("error.media_type");
    expect(checkMediaFile({ type: "image/png", size: MEDIA_MAX + 1 })).toBe("error.media_too_large");
  });
  it("does not call the API for an invalid file", async () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    const f = new File(["<svg/>"], "x.svg", { type: "image/svg+xml" });
    await expect(uploadMedia(f, "/client/creator/media")).rejects.toMatchObject({ code: "error.media_type" });
    expect(fetchSpy).not.toHaveBeenCalled();
  });
});
