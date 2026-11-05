import { describe, expect, it } from "vitest";
import { materialSnippet, nextTier } from "./affiliateExtras";

describe("affiliate extras", () => {
  it("builds escaped banner html and fills {link} in text", () => {
    expect(materialSnippet({ kind: "banner", title: 'A"B', body: "", image_url: "https://x/b.png" }, "https://s/r/abc")).toBe('<a href="https://s/r/abc"><img src="https://x/b.png" alt="A&quot;B" /></a>');
    expect(materialSnippet({ kind: "text", title: "", body: "Go {link} now", image_url: "" }, "U")).toBe("Go U now");
    expect(materialSnippet({ kind: "text", title: "", body: "Hosting", image_url: "" }, "U")).toBe("Hosting\nU");
  });
  it("finds the next tier", () => {
    const tiers = [{ id: "b", name: "Gold", min_referrals: 10, commission_percent: "20" }, { id: "a", name: "Silver", min_referrals: 3, commission_percent: "15" }];
    expect(nextTier(tiers, 0)?.name).toBe("Silver");
    expect(nextTier(tiers, 3)?.name).toBe("Gold");
    expect(nextTier(tiers, 10)).toBeNull();
  });
});
