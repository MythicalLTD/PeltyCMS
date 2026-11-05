// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLiveChat, type ChatSync } from "./useLiveChat";

type SocketEvent = "open" | "message" | "close" | "error";

class Socket {
  static OPEN = 1;
  static instances: Socket[] = [];
  readyState = 1;
  sent: string[] = [];
  private listeners = new Map<SocketEvent, (...args: unknown[]) => void>();
  constructor(public url: string) { Socket.instances.push(this); }
  addEventListener(type: string, handler: (...args: unknown[]) => void) {
    this.listeners.set(type as SocketEvent, handler);
  }
  send(data: string) { this.sent.push(data); }
  open() { this.listeners.get("open")?.(); }
  close() { this.readyState = 3; this.listeners.get("close")?.(); }
  receive(data: unknown) { this.listeners.get("message")?.({ data: JSON.stringify(data) }); }
}
beforeEach(() => { vi.useFakeTimers(); Socket.instances = []; vi.stubGlobal("WebSocket", Socket); });
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers(); });
describe("live chat websocket", () => {
  it("sends credentials in first frame, replays from cursor and reconnects", () => {
    let cursor = 4;
    const callback = vi.fn<(event: ChatSync) => void>();
    const { unmount } = renderHook(() => useLiveChat("/chat/sessions/id/ws", { token: "secret" }, callback, () => cursor));
    const socket = Socket.instances[0]!;
    expect(socket.url).not.toContain("secret");
    act(() => socket.open());
    expect(JSON.parse(socket.sent[0]!)).toEqual({ token: "secret", after: 4 });
    act(() => socket.receive({ type: "sync", status: "open", messages: [] }));
    expect(callback).toHaveBeenCalledOnce();
    cursor = 9;
    act(() => socket.close());
    act(() => vi.advanceTimersByTime(1000));
    const next = Socket.instances[1]!;
    act(() => next.open());
    expect(JSON.parse(next.sent[0]!).after).toBe(9);
    unmount();
    act(() => vi.advanceTimersByTime(60000));
    expect(Socket.instances).toHaveLength(2);
  });
  it("expires ephemeral presence and throttles typing", () => {
    const { result } = renderHook(() => useLiveChat("/admin/chats/id/ws", { csrf: "csrf" }, vi.fn<(event: ChatSync) => void>(), () => 0));
    const socket = Socket.instances[0]!;
    act(() => socket.receive({ type: "signal", connection: "peer", online: true, typing: true }));
    act(() => vi.advanceTimersByTime(1000));
    expect(result.current.online).toBe(true); expect(result.current.typing).toBe(true);
    act(() => { result.current.sendTyping(true); result.current.sendTyping(true); });
    expect(socket.sent).toHaveLength(1);
    act(() => vi.advanceTimersByTime(8000));
    expect(result.current.online).toBe(false); expect(result.current.typing).toBe(false);
  });
  it("does not connect without credentials", () => {
    renderHook(() => useLiveChat("/admin/chats/ws", {}, vi.fn<(event: ChatSync) => void>(), () => 0));
    expect(Socket.instances).toHaveLength(0);
  });
});
