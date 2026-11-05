import { describe, expect, it } from "vitest";
import { checkZipFile, registryState, MAX_ADDON_ZIP_BYTES } from "./addonPackages";

describe("addon packages", () => {
  it("prechecks zip files", () => {
    expect(checkZipFile(null)).toBe("addons.upload.pick");
    expect(checkZipFile({ name: "a.tar", size: 5 })).toBe("addons.upload.not_zip");
    expect(checkZipFile({ name: "a.zip", size: 0 })).toBe("addons.upload.empty");
    expect(checkZipFile({ name: "a.ZIP", size: MAX_ADDON_ZIP_BYTES + 1 })).toBe("error.addon_too_large");
    expect(checkZipFile({ name: "a.zip", size: 10 })).toBeNull();
  });
  it("derives registry state", () => {
    expect(registryState({ builtin: true, local_version: "1", version: "2" })).toBe("builtin");
    expect(registryState({ builtin: false, local_version: null, version: "1.0.0" })).toBe("new");
    expect(registryState({ builtin: false, local_version: "1.2.0", version: "1.10.0" })).toBe("update");
    expect(registryState({ builtin: false, local_version: "2.0.0", version: "2.0" })).toBe("current");
  });
});
