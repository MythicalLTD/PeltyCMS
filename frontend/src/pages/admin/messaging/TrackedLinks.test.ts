import { describe, expect, it } from "vitest";
import { feedSnippet, trackedUrl } from "./TrackedLinks";

describe("trackedUrl", () => {
  it("builds the public redirect URL", () => {
    expect(trackedUrl("promo", "https://billing.example")).toBe("https://billing.example/api/v1/go/promo");
  });
});

describe("feedSnippet", () => {
  it("builds product and TLD script tags", () => {
    expect(feedSnippet("product", "web-pro", "price", "eur", "annual", "https://b.x")).toBe('<script src="https://b.x/api/v1/feeds/products/web-pro?currency=EUR&field=price&cycle=annual"></script>');
    expect(feedSnippet("product", "web-pro", "name", "EUR", "annual", "https://b.x")).toBe('<script src="https://b.x/api/v1/feeds/products/web-pro?currency=EUR&field=name"></script>');
    expect(feedSnippet("tld", ".com", "price", "", "monthly", "https://b.x")).toBe('<script src="https://b.x/api/v1/feeds/tlds/com?currency=EUR"></script>');
  });
});
