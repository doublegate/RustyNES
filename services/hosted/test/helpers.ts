// SPDX-License-Identifier: GPL-3.0-or-later
//
// Test helpers: a signalling client over a real WebSocket into the lobby
// Durable Object, and polling waits (the runtime delivers frames
// asynchronously, so a test waits for a condition rather than sleeping a guess).

import { CLIENT_IP_HEADER } from "../src/env";

export interface TestClient {
  ws: WebSocket;
  inbox: string[];
  closed: () => boolean;
  send: (text: string) => void;
}

export function wrap(ws: WebSocket): TestClient {
  ws.accept();
  const inbox: string[] = [];
  let closed = false;
  ws.addEventListener("message", (e) => {
    inbox.push(String(e.data));
  });
  ws.addEventListener("close", () => {
    closed = true;
  });
  return { ws, inbox, closed: () => closed, send: (t) => ws.send(t) };
}

/** Open a socket straight into a lobby object (bypassing the router). */
export async function connectStub(stub: DurableObjectStub, ip: string): Promise<TestClient> {
  const resp = await stub.fetch("https://lobby.internal/signal", {
    headers: { Upgrade: "websocket", [CLIENT_IP_HEADER]: ip },
  });
  if (resp.status !== 101 || !resp.webSocket) throw new Error(`upgrade refused: ${resp.status}`);
  return wrap(resp.webSocket);
}

export async function waitFor(cond: () => boolean, what: string, timeoutMs = 2000): Promise<void> {
  const start = Date.now();
  while (!cond()) {
    if (Date.now() - start > timeoutMs) throw new Error(`timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 2));
  }
}

export const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
