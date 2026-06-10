// One zk-vm party in its own worker — its own, isolated wasm memory.
//
// The page spawns this file twice: once as the prover, once as the verifier.
// Each run request carries a transferred MessagePort; the wasm bindings speak
// the mpz protocol over it, and the page relays the bytes to the peer.

import init, {
  prover_age,
  prover_sha256,
  prover_square,
  verifier_age,
  verifier_sha256,
  verifier_square,
} from "./pkg/zkvm_demo.js";

export type Role = "prover" | "verifier";

export type PartyRequest =
  | { type: "run"; role: Role; program: "square"; x: number }
  | { type: "run"; role: Role; program: "age"; birthdate: string; today: number }
  | {
      type: "run";
      role: Role;
      program: "sha256";
      message: Uint8Array; // empty on the verifier side
      msgLen: number;
    };

export type PartyResponse =
  | { type: "ready" }
  | { type: "done"; result: string; ms: number }
  | { type: "error"; message: string };

const post = (msg: PartyResponse) => self.postMessage(msg);

const initialized = init().then(() => post({ type: "ready" }));

self.onmessage = async (ev: MessageEvent<PartyRequest>) => {
  await initialized;
  const msg = ev.data;
  if (msg.type !== "run") return;
  const port = ev.ports[0];
  if (!port) {
    post({ type: "error", message: "no MessagePort transferred" });
    return;
  }
  try {
    const start = performance.now();
    let result: string;
    switch (msg.program) {
      case "square":
        result = String(
          msg.role === "prover"
            ? await prover_square(port, msg.x)
            : await verifier_square(port),
        );
        break;
      case "age":
        result = String(
          msg.role === "prover"
            ? await prover_age(port, msg.birthdate, msg.today)
            : await verifier_age(port, msg.today),
        );
        break;
      case "sha256":
        result =
          msg.role === "prover"
            ? await prover_sha256(port, msg.message)
            : await verifier_sha256(port, msg.msgLen);
        break;
    }
    post({ type: "done", result, ms: performance.now() - start });
  } catch (e) {
    post({ type: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
