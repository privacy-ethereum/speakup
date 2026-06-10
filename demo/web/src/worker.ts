// Web worker hosting the zk-vm wasm.
//
// v0: a single instance runs both parties (the bindings join them over an
// in-memory duplex). The message protocol is already shaped for the next
// step — one worker per party with a MessageChannel transport — so the UI
// won't change when the split happens.

import init, { square_zkvm } from "./pkg/zkvm_demo.js";

export type WorkerRequest = { type: "run"; program: "square"; x: number };

export type WorkerResponse =
  | { type: "ready" }
  | { type: "done"; result: number; ms: number }
  | { type: "error"; message: string };

const post = (msg: WorkerResponse) => self.postMessage(msg);

const initialized = init().then(() => post({ type: "ready" }));

self.onmessage = async (ev: MessageEvent<WorkerRequest>) => {
  await initialized;
  const msg = ev.data;
  if (msg.type !== "run") return;
  try {
    const start = performance.now();
    const result = await square_zkvm(msg.x);
    post({ type: "done", result, ms: performance.now() - start });
  } catch (e) {
    post({ type: "error", message: String(e) });
  }
};
