// @vitest-environment jsdom
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLiveChat } from "./useLiveChat";

class Socket {
  static OPEN = 1;
  static instances: Socket[] = [];
  readyState = 1;
  onopen: (() => void) | null = null;
  onmessage: ((e: { data: string }) => void) | null = null;
  onclose: (() => void) | null = null;
  onerror: (() => void) | null = null;
  sent: string[] = [];
  constructor(public url: string) { Socket.instances.push(this); }
  send(data: string) { this.sent.push(data); }
  close() { this.readyState = 3; this.onclose?.(); }
  receive(data: unknown) { this.onmessage?.({ data: JSON.stringify(data) }); }
}
beforeEach(() => { vi.useFakeTimers(); Socket.instances = []; vi.stubGlobal("WebSocket", Socket); });
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.useRealTimers(); });
describe("live chat websocket", () => {
  it("sends credentials in first frame, replays from cursor and reconnects", () => {
    let cursor = 4;
    const callback = vi.fn();
    const { unmount } = renderHook(() => useLiveChat("/chat/sessions/id/ws", { token: "secret" }, callback, () => cursor));
    const socket = Socket.instances[0]!;
    expect(socket.url).not.toContain("secret");
    act(() => socket.onopen?.());
    expect(JSON.parse(socket.sent[0]!)).toEqual({ token: "secret", after: 4 });
    act(() => socket.receive({ type: "sync", status: "open", messages: [] }));
    expect(callback).toHaveBeenCalledOnce();
    cursor = 9;
    act(() => socket.close());
    act(() => vi.advanceTimersByTime(1000));
    const next = Socket.instances[1]!;
    act(() => next.onopen?.());
    expect(JSON.parse(next.sent[0]!).after).toBe(9);
    unmount();
    act(() => vi.advanceTimersByTime(60000));
    expect(Socket.instances).toHaveLength(2);
  });
  it("expires ephemeral presence and throttles typing", () => {
    const { result } = renderHook(() => useLiveChat("/admin/chats/id/ws", { csrf: "csrf" }, vi.fn(), () => 0));
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
    renderHook(() => useLiveChat("/admin/chats/ws", {}, vi.fn(), () => 0));
    expect(Socket.instances).toHaveLength(0);
  });
});
