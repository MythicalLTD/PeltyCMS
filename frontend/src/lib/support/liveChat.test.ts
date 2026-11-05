import { describe, expect, it } from "vitest";
import {
  lastId,
  loadCreds,
  mergeMessages,
  validBody,
  CHAT_KEY,
  endMinutesToTime,
  isSafeChatHref,
  minutesToTime,
  parseChatAction,
  timeToEndMinutes,
  timeToMinutes,
  type ChatMessage,
} from "./liveChat";

const m = (id: number): ChatMessage => ({ id, author: "visitor", staff_name: null, body: "x", created_at: "" });
const store = (v: string | null) => ({ getItem: (k: string) => (k === CHAT_KEY ? v : null) });

describe("liveChat", () => {
  it("merges without duplicates in id order", () => {
    expect(mergeMessages([m(1), m(2)], [m(2), m(4), m(3)]).map((x) => x.id)).toEqual([1, 2, 3, 4]);
    expect(lastId([])).toBe(0);
    expect(lastId([m(1), m(7)])).toBe(7);
  });
  it("loads only well-formed creds", () => {
    expect(loadCreds(store(JSON.stringify({ id: "a", token: "f".repeat(64) })))).toEqual({ id: "a", token: "f".repeat(64) });
    expect(loadCreds(store(JSON.stringify({ id: "a", token: "short" })))).toBeNull();
    expect(loadCreds(store("{bad"))).toBeNull();
    expect(loadCreds(store(null))).toBeNull();
  });
  it("validates body", () => {
    expect(validBody("  ")).toBe(false);
    expect(validBody("hi")).toBe(true);
    expect(validBody("a".repeat(2000))).toBe(true);
    expect(validBody("a".repeat(2001))).toBe(false);
    expect(validBody("😀".repeat(2000))).toBe(true);
    expect(validBody("😀".repeat(2001))).toBe(false);
  });
  it("converts opening-hour minutes and time inputs", () => {
    expect(minutesToTime(540)).toBe("09:00");
    expect(minutesToTime(1020)).toBe("17:00");
    expect(timeToMinutes("09:00")).toBe(540);
    expect(timeToMinutes("17:00")).toBe(1020);
    expect(endMinutesToTime(1440)).toBe("00:00");
    expect(endMinutesToTime(1020)).toBe("17:00");
    expect(timeToEndMinutes("00:00")).toBe(1440);
    expect(timeToEndMinutes("17:00")).toBe(1020);
  });
  it("parses safe suggestion actions and rejects unsafe hrefs", () => {
    expect(isSafeChatHref("/store")).toBe(true);
    expect(isSafeChatHref("//evil.test")).toBe(false);
    expect(isSafeChatHref("https://evil.test")).toBe(false);
    const id = "00000000-0000-0000-0000-0000000000aa";
    expect(parseChatAction(JSON.stringify({ kind: "open_product", href: `/store/p/${id}`, label: "Starter" }))).toEqual({
      kind: "open_product",
      href: `/store/p/${id}`,
      label: "Starter",
    });
    expect(parseChatAction(JSON.stringify({ kind: "open_page", href: "/page/about", label: "About" }))?.href).toBe("/page/about");
    expect(parseChatAction("closed_by_staff")).toBeNull();
    expect(parseChatAction(JSON.stringify({ kind: "open_page", href: "//evil.example", label: "Evil" }))).toBeNull();
  });
});
