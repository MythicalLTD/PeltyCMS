import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { onlineManager } from "@tanstack/react-query";
import { ApiError } from "./api";
import { isUnreachableError, noteApiReachable, noteApiUnreachable, resetApiHealth, setHealthFetch } from "./api-health";

describe("isUnreachableError", () => {
  it("matches gateway statuses and failed fetch", () => {
    expect(isUnreachableError(new ApiError(503, "error.unavailable", "down"))).toBe(true);
    expect(isUnreachableError(new ApiError(502, "error.internal", "bad gateway"))).toBe(true);
    expect(isUnreachableError(new ApiError(500, "error.unavailable", "proxy"))).toBe(true);
    expect(isUnreachableError(new TypeError("Failed to fetch"))).toBe(true);
    expect(isUnreachableError(new ApiError(500, "error.internal", "bug", "req-1"))).toBe(false);
    expect(isUnreachableError(new ApiError(404, "error.not_found", "missing"))).toBe(false);
  });
});

describe("api health circuit", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetApiHealth();
  });
  afterEach(() => {
    resetApiHealth();
    setHealthFetch((...args) => globalThis.fetch(...args));
    vi.useRealTimers();
  });

  it("goes offline on unreachable and recovers after a healthy probe", async () => {
    const probe = vi.fn()
      .mockResolvedValueOnce(new Response(null, { status: 503 }))
      .mockResolvedValueOnce(new Response("ok", { status: 200 }));
    setHealthFetch(probe as unknown as typeof fetch);

    noteApiUnreachable();
    expect(onlineManager.isOnline()).toBe(false);

    await vi.advanceTimersByTimeAsync(2_000);
    expect(probe).toHaveBeenCalledTimes(1);
    expect(onlineManager.isOnline()).toBe(false);

    await vi.advanceTimersByTimeAsync(4_000);
    expect(probe).toHaveBeenCalledTimes(2);
    expect(onlineManager.isOnline()).toBe(true);
  });

  it("noteApiReachable restores online immediately", () => {
    noteApiUnreachable();
    expect(onlineManager.isOnline()).toBe(false);
    noteApiReachable();
    expect(onlineManager.isOnline()).toBe(true);
  });
});
