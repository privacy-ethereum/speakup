// Web worker hosting the zk-vm wasm.
//
// Both parties run in this one instance over the REAL OT stack
// (Chou-Orlandi → KOS → Ferret); the next milestone moves each party into
// its own worker with a MessageChannel transport. The message protocol is
// already shaped for that split.

import init, { age_zkvm, sha256_zkvm, square_zkvm } from "./pkg/zkvm_demo.js";

export type WorkerRequest =
  | { type: "run"; program: "square"; x: number }
  | { type: "run"; program: "age"; birthdate: string; today: number }
  | { type: "run"; program: "sha256"; message: Uint8Array };

export type WorkerResponse =
  | { type: "ready" }
  | { type: "done"; result: string; ms: number }
  | { type: "error"; message: string };

const post = (msg: WorkerResponse) => self.postMessage(msg);

const initialized = init().then(() => post({ type: "ready" }));

self.onmessage = async (ev: MessageEvent<WorkerRequest>) => {
  await initialized;
  const msg = ev.data;
  if (msg.type !== "run") return;
  try {
    const start = performance.now();
    let result: string;
    switch (msg.program) {
      case "square":
        result = String(await square_zkvm(msg.x));
        break;
      case "age":
        result = String(await age_zkvm(msg.birthdate, msg.today));
        break;
      case "sha256":
        result = await sha256_zkvm(msg.message);
        break;
    }
    post({ type: "done", result, ms: performance.now() - start });
  } catch (e) {
    post({ type: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
