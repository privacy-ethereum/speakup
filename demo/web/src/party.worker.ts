// One zk-vm party in its own worker — its own, isolated wasm memory.
//
// The page spawns this file twice: once as the prover, once as the verifier.
// Each run request carries a transferred MessagePort and an orchestration
// script (authored in the page's editor). The worker builds a raw `Party` over
// the port from the speakup-wasm bindings, wraps it in the demo's role-aware
// `Vm` facade, then runs the script against that. Both parties run the SAME
// script; only the inputs (and, hidden inside `Vm`, the role) differ.

import { Private, Public, Vm, helpers, makeGuest, type ExportInfo, type Param, type Role } from "./zkvm";

export type { ExportInfo, Role };

// The wasm pkg is served as plain files (web/public/pkg), NOT bundled: it is
// built with web-spawn's `no-bundler` glue, which resolves the module and the
// nested thread workers by URL at runtime. Letting vite bundle it instead
// inlines the nested worker as a data: URL, whose import.meta.url can't
// resolve anything — threads would break in production.
type Pkg = typeof import("../public/pkg/speakup_wasm");

// ?v=<pkg content hash>: the pkg files keep stable names and GitHub Pages
// caches them for 10 minutes, so without this a warm browser would pair
// freshly-deployed page code with the previous deploy's glue/wasm.
const pkgUrl = new URL(
  `${import.meta.env.BASE_URL}pkg/speakup_wasm.js?v=${__PKG_VERSION__}`,
  self.location.origin,
).href;
// Passed to init explicitly: the glue's own fallback resolves the wasm
// relative to import.meta.url, which would drop the version query.
const wasmUrl = new URL(
  `${import.meta.env.BASE_URL}pkg/speakup_wasm_bg.wasm?v=${__PKG_VERSION__}`,
  self.location.origin,
);

/// A run request: the orchestration script, the guest module bytes, and the
/// inputs. `pub` reaches both parties. `priv` reaches both too, but only the
/// prover's carries real secret bytes — the verifier's are a same-length
/// placeholder (it only ever needs the length, for the blind write). `role`
/// selects which party to build; the script itself never sees it.
export interface RunRequest {
  type: "run";
  role: Role;
  script: string;
  wasm: Uint8Array;
  pub: Record<string, unknown>;
  priv: Record<string, unknown>;
  /// The configured scalar arguments (public/private per the panel), ready to
  /// spread into a `guest` call. Private values are zeroed for the verifier.
  args: Param[];
}

/// Parse a module's exported functions (no protocol, no port) so the page can
/// build the typed `guest` API, autocomplete, and the argument panel.
export interface InspectRequest {
  type: "inspect";
  wasm: Uint8Array;
}

export type PartyRequest = RunRequest | InspectRequest;

export type PartyResponse =
  | { type: "ready" }
  | { type: "done"; result: string; ms: number }
  | { type: "error"; message: string }
  | { type: "exports"; exports: ExportInfo[] }
  | { type: "exports-error"; message: string };

const post = (msg: PartyResponse) => self.postMessage(msg);

let pkg: Pkg;
let initError: string | null = null;
const initialized = (async () => {
  try {
    pkg = await import(/* @vite-ignore */ pkgUrl);
    await pkg.default({ module_or_path: wasmUrl });
    // Threading: a web-spawn spawner plus a rayon pool, all nested workers
    // of this one (terminated with it on abort). Half the cores per party —
    // the prover and verifier run simultaneously on this machine.
    const threads = Math.max(
      1,
      Math.floor((navigator.hardwareConcurrency || 4) / 2),
    );
    await pkg.initialize(threads);
    post({ type: "ready" });
  } catch (e) {
    initError = e instanceof Error ? e.message : String(e);
    post({
      type: "error",
      message: `wasm failed to load: ${initError} — try a hard reload (Cmd/Ctrl+Shift+R)`,
    });
  }
})();

/// The exported function names of `wasm`, for the `guest` facade.
const exportNames = (wasm: Uint8Array): string[] =>
  (JSON.parse(pkg.module_exports(wasm)) as ExportInfo[]).map((e) => e.name);

self.onmessage = async (ev: MessageEvent<PartyRequest>) => {
  await initialized;
  if (initError) return; // already reported; nothing can run
  const msg = ev.data;

  if (msg.type === "inspect") {
    try {
      post({ type: "exports", exports: JSON.parse(pkg.module_exports(msg.wasm)) });
    } catch (e) {
      post({ type: "exports-error", message: e instanceof Error ? e.message : String(e) });
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
    const party =
      msg.role === "prover"
        ? pkg.Party.prover(port, msg.wasm)
        : pkg.Party.verifier(port, msg.wasm);
    const vm = new Vm(party, msg.role);
    // The typed API for this module: `guest.<export>(...params)`.
    const guest = makeGuest(vm, exportNames(msg.wasm));
    // The orchestration is user-authored JS (from the page's editor). It runs
    // in this worker's isolated wasm memory; this is a local, serverless demo,
    // so the only code it can touch is the visitor's own.
    const run = new Function(
      "vm",
      "guest",
      "args",
      "pub",
      "priv",
      "Public",
      "Private",
      "helpers",
      `"use strict";\nreturn (async () => {\n${msg.script}\n})();`,
    ) as (
      vm: Vm,
      guest: ReturnType<typeof makeGuest>,
      args: Param[],
      pub: Record<string, unknown>,
      priv: Record<string, unknown>,
      pub_: typeof Public,
      priv_: typeof Private,
      h: typeof helpers,
    ) => Promise<unknown>;
    const result = await run(vm, guest, msg.args, msg.pub, msg.priv, Public, Private, helpers);
    post({ type: "done", result: String(result ?? ""), ms: performance.now() - start });
  } catch (e) {
    post({ type: "error", message: e instanceof Error ? e.message : String(e) });
  }
};
