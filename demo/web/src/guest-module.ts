// The guest-module domain: the module type, AssemblyScript compilation, the
// starter-orchestration generator, the module-info line renderer, fetching a
// built-in, and the one-at-a-time export inspector. No view ownership beyond the
// single `module-info` text line.

import { fmtBytes, sha256Hex } from "./dom";
import type { ExportInfo } from "./zkvm";

/// A guest module: a built-in example's .wasm, a dropped upload, or a freshly
/// compiled AssemblyScript binary. `exports` is null until the inspector fills it.
export interface GuestModule {
  name: string;
  wasm: Uint8Array;
  builtin: boolean;
  exports: ExportInfo[] | null;
}

export const CUSTOM_WASM_CAP = 4 << 20; // 4 MB — guests are tens of KB

// --- AssemblyScript compilation (lazy: the compiler loads on first use) ---

type Asc = (typeof import("assemblyscript/asc"))["default"];
let asc: Asc | null = null;

/// Compile AssemblyScript source to wasm bytes; throws on a compile error
/// (message + stderr). Shared by the custom tab and the remote verifier, which
/// recompiles the prover's source to check it against the proven binary.
export const compileAscSource = async (source: string): Promise<Uint8Array> => {
  asc ??= (await import("assemblyscript/asc")).default;
  const { error, stderr, binary } = await asc.compileString(source, {
    optimizeLevel: 3,
    runtime: "stub",
  });
  if (error || !binary) {
    throw new Error(
      `${error?.message ?? "compile failed"}\n${String(stderr)}`.trim(),
    );
  }
  return new Uint8Array(binary);
};

/// A starter orchestration listing a module's exports and calling the first
/// scalar one over a private argument — so a freshly compiled/dropped module
/// runs immediately.
export const customStarter = (m: GuestModule): string => {
  const supported = (m.exports ?? []).filter((e) => e.supported);
  const lines = supported
    .map((e) => `//   ${e.name}(${e.params.join(", ")})${e.results.length ? ` -> ${e.results.join(",")}` : ""}`)
    .join("\n");
  const first = supported[0];
  // First argument is the prover's secret; any further arguments are public.
  // Sensible placeholders: a secret of 42 within public bounds 18..65 (so the
  // default inRange(42, 18, 65) reads as "is my age in [18, 65]?" → yes).
  const publicVals = [18, 65, 100, 1, 7];
  const arg = (i: number) =>
    i === 0 ? "Private(42)" : `Public(${publicVals[(i - 1) % publicVals.length]})`;
  const call = first
    ? `return await guest.${first.name}(${first.params.map((_, i) => arg(i)).join(", ")});`
    : `return "no callable i32/i64 exports";`;
  return `// Loaded ${m.name}. Exported functions:
${lines || "//   (no callable i32/i64 exports)"}
//
// Orchestrate against the typed \`guest\` API: Private(x) is the prover's secret,
// Public(x) is shared with both parties.
${call}
`;
};

/// Render the single module-info line (name · size · hash · export count).
export const renderModuleInfo = async (
  el: HTMLElement,
  m: GuestModule,
): Promise<void> => {
  const hash = (await sha256Hex(m.wasm)).slice(0, 8);
  const n = m.exports ? m.exports.filter((e) => e.supported).length : 0;
  const note = m.exports ? ` · ${n} callable export(s)` : " · inspecting…";
  el.textContent = `${m.name} · ${fmtBytes(m.wasm.length)} · sha-256 ${hash}…${note}`;
};

/// Fetch a built-in guest .wasm from under the deploy base URL.
export const fetchWasm = async (url: string): Promise<Uint8Array> => {
  const res = await fetch(`${import.meta.env.BASE_URL}${url}`);
  if (!res.ok) {
    throw new Error(
      `guest wasm not found (HTTP ${res.status}) — run \`npm run build:guest\``,
    );
  }
  return new Uint8Array(await res.arrayBuffer());
};

// --- the export inspector (one inspect in flight at a time) ---

/// How the inspector reaches the prover worker. The run controller owns the
/// workers, so it supplies this; the inspector stays a dumb queue.
export interface InspectHost {
  postInspect(wasm: Uint8Array): void;
}

export interface Inspector {
  /// Queue a module for export inspection.
  inspect(m: GuestModule): void;
  /// Forwarded from the prover worker by the run controller.
  onExports(exports: ExportInfo[]): void;
  onExportsError(message: string): void;
}

/// The prover worker inspects one module at a time; queue so concurrent requests
/// (e.g. several built-ins loading at once) don't clobber the pending one.
export const initInspector = (opts: {
  host: InspectHost;
  onResolved(m: GuestModule): void;
  onError(message: string): void;
}): Inspector => {
  let pending: GuestModule | null = null;
  const queue: GuestModule[] = [];

  const drain = () => {
    if (pending || queue.length === 0) return;
    pending = queue.shift()!;
    opts.host.postInspect(pending.wasm);
  };

  return {
    inspect(m) {
      queue.push(m);
      drain();
    },
    onExports(exports) {
      const m = pending;
      pending = null;
      if (m) {
        m.exports = exports;
        opts.onResolved(m);
      }
      drain();
    },
    onExportsError(message) {
      pending = null;
      opts.onError(message);
      drain();
    },
  };
};
