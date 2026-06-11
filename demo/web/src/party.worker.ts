// One zk-vm party in its own worker — its own, isolated wasm memory.
//
// The page spawns this file twice: once as the prover, once as the verifier.
// Each run request carries a transferred MessagePort; the wasm bindings speak
// the mpz protocol over it, and the page relays the bytes to the peer.

// The wasm pkg is served as plain files (web/public/pkg), NOT bundled: it is
// built with web-spawn's `no-bundler` glue, which resolves the module and the
// nested thread workers by URL at runtime. Letting vite bundle it instead
// inlines the nested worker as a data: URL, whose import.meta.url can't
// resolve anything — threads would break in production.
type Pkg = typeof import("../public/pkg/zkvm_demo");

const pkgUrl = new URL(
  `${import.meta.env.BASE_URL}pkg/zkvm_demo.js`,
  self.location.origin,
).href;

export type Role = "prover" | "verifier";

/// Programs with a fixed, embedded guest module (everything but `custom`).
export type EmbeddedProgram =
  | "square"
  | "age"
  | "sha256"
  | "regex"
  | "luhn"
  | "csv";

export type PartyRequest =
  | { type: "guest_info"; program: EmbeddedProgram }
  | { type: "inspect"; wasm: Uint8Array }
  | {
      type: "run";
      role: Role;
      program: "custom";
      wasm: Uint8Array;
      func: string;
      vis: Uint8Array; // 1 where the argument is the prover's private input
      values: BigInt64Array; // ignored at private positions on the verifier
    }
  | { type: "run"; role: Role; program: "square"; x: number }
  | { type: "run"; role: Role; program: "age"; birthdate: string; today: number }
  | {
      type: "run";
      role: Role;
      program: "sha256";
      message: Uint8Array; // empty on the verifier side
      msgLen: number;
    }
  | {
      type: "run";
      role: Role;
      program: "regex";
      pattern: string; // public: both sides get it
      text: string; // empty on the verifier side
      textLen: number;
    }
  | {
      type: "run";
      role: Role;
      program: "luhn";
      number: string; // empty on the verifier side
      numLen: number;
    }
  | {
      type: "run";
      role: Role;
      program: "csv";
      csv: string; // empty on the verifier side
      len: number;
      col: number; // public: both sides get them
      threshold: number;
    };

/// One exported function of an inspected module.
export interface ExportInfo {
  name: string;
  params: string[]; // "i32" | "i64" | "f32" | "f64"
  results: string[];
  supported: boolean;
}

export type PartyResponse =
  | { type: "ready" }
  | { type: "done"; result: string; ms: number }
  | { type: "error"; message: string }
  | { type: "guest_info"; program: EmbeddedProgram; size: number; sha256: string }
  | { type: "exports"; exports?: ExportInfo[]; error?: string };

const post = (msg: PartyResponse) => self.postMessage(msg);

let pkg: Pkg;
const initialized = (async () => {
  pkg = await import(/* @vite-ignore */ pkgUrl);
  await pkg.default();
  // Threading: a web-spawn spawner plus a rayon pool, all nested workers of
  // this one (terminated with it on abort). Half the cores per party — the
  // prover and verifier run simultaneously on this machine.
  const threads = Math.max(
    1,
    Math.floor((navigator.hardwareConcurrency || 4) / 2),
  );
  await pkg.initialize(threads);
  post({ type: "ready" });
})();

self.onmessage = async (ev: MessageEvent<PartyRequest>) => {
  await initialized;
  const msg = ev.data;
  if (msg.type === "guest_info") {
    // Facts about this party's own embedded module — the page shows both
    // parties' answers side by side, so "same module, same hash" is
    // something visitors can check, not just read.
    const bytes = pkg.guest_wasm(msg.program);
    const digest = await crypto.subtle.digest("SHA-256", bytes.buffer as ArrayBuffer);
    const sha256 = [...new Uint8Array(digest)]
      .map((b) => b.toString(16).padStart(2, "0"))
      .join("");
    post({ type: "guest_info", program: msg.program, size: bytes.length, sha256 });
    return;
  }
  if (msg.type === "inspect") {
    // Parse the user's module and report its exported functions, so the
    // page can build the argument UI from the real signatures.
    try {
      post({ type: "exports", exports: JSON.parse(pkg.module_exports(msg.wasm)) });
    } catch (e) {
      post({
        type: "exports",
        error: e instanceof Error ? e.message : String(e),
      });
    }
    return;
  }
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
            ? await pkg.prover_square(port, msg.x)
            : await pkg.verifier_square(port),
        );
        break;
      case "age":
        result = String(
          msg.role === "prover"
            ? await pkg.prover_age(port, msg.birthdate, msg.today)
            : await pkg.verifier_age(port, msg.today),
        );
        break;
      case "sha256":
        result =
          msg.role === "prover"
            ? await pkg.prover_sha256(port, msg.message)
            : await pkg.verifier_sha256(port, msg.msgLen);
        break;
      case "regex":
        result = String(
          msg.role === "prover"
            ? await pkg.prover_regex(port, msg.pattern, msg.text)
            : await pkg.verifier_regex(port, msg.pattern, msg.textLen),
        );
        break;
      case "custom":
        result =
          msg.role === "prover"
            ? await pkg.prover_custom(port, msg.wasm, msg.func, msg.vis, msg.values)
            : await pkg.verifier_custom(port, msg.wasm, msg.func, msg.vis, msg.values);
        break;
      case "luhn":
        result = String(
          msg.role === "prover"
            ? await pkg.prover_luhn(port, msg.number)
            : await pkg.verifier_luhn(port, msg.numLen),
        );
        break;
      case "csv":
        result = String(
          msg.role === "prover"
            ? await pkg.prover_csv(port, msg.csv, msg.col, msg.threshold)
            : await pkg.verifier_csv(port, msg.len, msg.col, msg.threshold),
        );
        break;
    }
    post({ type: "done", result, ms: performance.now() - start });
  } catch (e) {
    post({ type: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
