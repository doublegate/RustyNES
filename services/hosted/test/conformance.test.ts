// SPDX-License-Identifier: GPL-3.0-or-later
//
// Signalling conformance: the hosted lobby against the reference Relay.
//
// `vectors/signaling-vectors.json` is generated FROM the Rust reference
// (crates/rustynes-netplay/tests/hosted_signaling_vectors.rs, which also fails
// if the committed file stops matching the Rust code). Each scenario is a
// sequence of inbound frames and disconnects with the exact outbound frames and
// closes the reference produces. Here it is replayed three ways:
//
//   1. against the pure TypeScript Relay (protocol.ts);
//   2. end to end over real WebSockets into the Lobby Durable Object;
//   3. the same, evicting the Durable Object (hibernating its sockets) before
//      every sending step, so each of those steps runs on a relay rebuilt from
//      the sockets' attachments. This is the test of the hibernation design.
//
// Frames are compared as exact strings, which is what "byte-compatible" means
// to a client.

import { env } from "cloudflare:workers";
import { evictDurableObject } from "cloudflare:test";
import { describe, expect, it } from "vitest";
import { type Action, Relay, encodeSignal, parseSignal } from "../src/protocol";
import { type TestClient, connectStub, sleep, waitFor } from "./helpers";
import vectors from "./vectors/signaling-vectors.json";

type Expect = { send: { to: number; text: string } } | { close: number };
interface Step {
  op: "send" | "disconnect";
  client: number;
  text?: string;
  expect: Expect[];
}
interface Scenario {
  name: string;
  steps: Step[];
}
const scenarios = (vectors as { scenarios: Scenario[] }).scenarios;

function render(a: Action): Expect {
  return a.kind === "send" ? { send: { to: a.to, text: encodeSignal(a.msg) } } : { close: a.who };
}

describe("vectors", () => {
  it("cover the scenarios the Rust generator wrote", () => {
    expect(scenarios.length).toBeGreaterThanOrEqual(11);
    expect(scenarios.reduce((n, s) => n + s.steps.length, 0)).toBeGreaterThanOrEqual(60);
  });
});

describe("pure relay matches the reference", () => {
  for (const sc of scenarios) {
    it(sc.name, () => {
      const relay = new Relay();
      sc.steps.forEach((step, i) => {
        let got: Expect[] = [];
        if (step.op === "send") {
          const msg = parseSignal(step.text!);
          got = msg ? relay.handle(step.client, msg).map(render) : [];
        } else {
          got = relay.disconnect(step.client).map(render);
        }
        expect(got, `step ${i}: ${step.text ?? "disconnect " + step.client}`).toEqual(step.expect);
      });
    });
  }
});

/**
 * Replay a scenario over WebSockets. Client ids in the vectors are mapped to
 * sockets; each socket's inbox must equal, in order, the frames the vectors
 * send it, and a `close` action must close it.
 */
async function replay(sc: Scenario, objectName: string, evictEachStep: boolean): Promise<void> {
  const stub = env.LOBBY.getByName(objectName);
  const clients = new Map<number, TestClient>();
  const want = new Map<number, string[]>();
  const wantClosed = new Set<number>();
  const sid = Math.floor(Math.random() * 200) + 1;
  for (const [i, step] of sc.steps.entries()) {
    let c = clients.get(step.client);
    if (!c && step.op === "send") {
      // A distinct address per client keeps the per-IP limits out of the way.
      c = await connectStub(stub, `10.${sid}.${i % 250}.${step.client}`);
      clients.set(step.client, c);
    }
    if (step.op === "send") c!.send(step.text!);
    else c!.ws.close(1000, "bye");
    for (const e of step.expect) {
      if ("send" in e) {
        const list = want.get(e.send.to) ?? [];
        list.push(e.send.text);
        want.set(e.send.to, list);
      } else {
        wantClosed.add(e.close);
      }
    }
    // Wait until every client has everything expected so far.
    await waitFor(
      () => [...want].every(([id, list]) => (clients.get(id)?.inbox.length ?? 0) >= list.length),
      `step ${i} deliveries`,
    );
    for (const id of wantClosed) await waitFor(() => clients.get(id)!.closed(), `close of ${id}`);
    // A step with no visible effect still has to be processed before the
    // next one; give the object a moment (socket order covers the rest).
    if (step.expect.length === 0) await sleep(25);
    // Evict before every step that SENDS. Not before a disconnect: in the
    // local workerd test runtime a client close does not wake a hibernated
    // object (measured 2026-10-16: the close event never arrived within 4 s,
    // while a message after the same eviction did). Production documents the
    // close as delivered after hibernation; the non-evicting replay above
    // covers the disconnect path.
    const next = sc.steps[i + 1];
    if (evictEachStep && next?.op === "send") await evictDurableObject(stub);
  }
  await sleep(25);
  for (const [id, c] of clients) {
    expect(c.inbox, `frames received by client ${id}`).toEqual(want.get(id) ?? []);
    expect(c.closed(), `client ${id} closed`).toBe(wantClosed.has(id) || sc.steps.some((s) => s.op === "disconnect" && s.client === id));
  }
  for (const c of clients.values()) if (!c.closed()) c.ws.close(1000, "done");
}

describe("lobby Durable Object matches the reference over WebSockets", () => {
  scenarios.forEach((sc, si) => {
    it(sc.name, async () => {
      await replay(sc, `conformance-${si}`, false);
    });
  });
});

describe("lobby keeps matching the reference across hibernation", () => {
  scenarios.forEach((sc, si) => {
    it(sc.name, async () => {
      await replay(sc, `conformance-evict-${si}`, true);
    });
  });
});
