import { describe, expect, it } from "vitest";
import { initials } from "./UserAvatar";
import { checkAvatarFile } from "./ProfilePictureCard";

describe("avatars", () => {
  it("derives initials from names and emails", () => {
    expect(initials("Ana Maria Pop", "x@y.ro")).toBe("AP");
    expect(initials("ana", null)).toBe("AN");
    expect(initials("", "ion@example.com")).toBe("IO");
    expect(initials(null, null)).toBe("?");
  });
  it("pre-checks type and size", () => {
    expect(checkAvatarFile({ type: "image/gif", size: 10 })).toBe("error.avatar_type");
    expect(checkAvatarFile({ type: "image/png", size: 3 * 1024 * 1024 })).toBe("error.avatar_too_large");
    expect(checkAvatarFile({ type: "image/webp", size: 1000 })).toBeNull();
  });
});
