import { FEATURES } from "./config";
import type { PartyRequest, PartyResponse, Role } from "./party.worker";
import type { ExportInfo } from "./zkvm";
import { EXAMPLES, exampleForm, exampleTab, type MountedForm } from "./examples";
import { initTabs } from "./tabs";
import { initTooltips } from "./tooltip";
import "./style.css";

import { EditorView, basicSetup } from "codemirror";
import { javascript, javascriptLanguage } from "@codemirror/lang-javascript";
import type { Completion, CompletionContext, CompletionResult } from "@codemirror/autocomplete";

// One worker per party: two isolated wasm memories, so the secret physically
// cannot be in the other's address space. Spawned (and re-spawned on abort) by
// `spawnWorker`. The page relays their messages and counts the traffic.
const workers = {} as Record<Role, Worker>;

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const runBtn = $<HTMLButtonElement>("run");
const statPv = $("stat-pv");
const statVp = $("stat-vp");
const statTime = $("stat-time");
const resultBox = $("result-box");
const resultEl = $("result");
const cheatBtn = $<HTMLButtonElement>("cheat");

const guestTabs = $("guest-tabs");
const guestBody = $("guest-body");
const customConfig = $("custom-config");
const editorWrap = $("editor-wrap");
const customModeEl = $("custom-mode");
const ascPane = $("asc-pane");
const uploadPane = $("upload-pane");
const dropzone = $("dropzone");
const wasmFileInput = $<HTMLInputElement>("wasm-file");
const moduleInfo = $("module-info");

const utf8len = (s: string) => new TextEncoder().encode(s).length;
const fmtBytes = (n: number) =>
  n < 1024 ? `${n} B` : n < 1 << 20 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`;
const fmtMs = (ms: number) => (ms < 1000 ? `${ms.toFixed(0)} ms` : `${(ms / 1000).toFixed(2)} s`);

// --- orchestration scripts (per example, plus the custom tab) ---

const CUSTOM_SCRIPT = `// Drop a .wasm module above, then orchestrate it here against the typed
// \`guest\` API (autocomplete lists its exports once a module loads). Example:
//
//   const ptr = await guest.cabi_realloc(Public(0), Public(0), Public(1), Public(64));
//   vm.writePrivate(ptr, priv.message);
//   const out = await guest.hash(Public(ptr), Public(0));
//   return helpers.hex(vm.read(out, 32));

return "load a module and write your orchestration";
`;

const scripts: Record<string, string> = {
  ...Object.fromEntries(EXAMPLES.map((ex) => [ex.id, ex.script])),
  custom: CUSTOM_SCRIPT,
};

// --- the editor (CodeMirror) ---

const API_COMPLETIONS = [
  { label: "vm.callLocal", type: "method", detail: "(name, params) → number", info: "Call a local (non-interactive) export — e.g. an allocator or pointer getter." },
  { label: "vm.call", type: "method", detail: "(name, params) → Promise<number>", info: "Call an interactive export (returns a Promise)." },
  { label: "vm.writePrivate", type: "method", detail: "(ptr, bytes)", info: "Stage a private input at ptr. The prover contributes the bytes; the verifier blinds by their length." },
  { label: "vm.writePublic", type: "method", detail: "(ptr, bytes)", info: "Stage public bytes (known to both) at ptr." },
  { label: "vm.read", type: "method", detail: "(ptr, len) → Uint8Array", info: "Read len revealed bytes at ptr." },
  { label: "Public", type: "function", detail: '(value, ty = "i32") → Param', info: "A public call argument (known to both parties)." },
  { label: "Private", type: "function", detail: '(value, ty = "i32") → Param', info: "A private call argument: the prover contributes the value, the verifier blinds it automatically." },
  { label: "helpers.hex", type: "function", detail: "(bytes) → string", info: "Lowercase hex of a byte array." },
  { label: "helpers.utf8", type: "function", detail: "(string) → Uint8Array" },
  { label: "helpers.text", type: "function", detail: "(bytes) → string" },
  { label: "pub", type: "variable", info: "Public inputs (both parties)." },
  { label: "priv", type: "variable", info: "Private inputs (the prover's secret)." },
  { label: "guest", type: "variable", info: "The typed API for the loaded module — one method per export." },
] satisfies Completion[];

// Per-module completions for the typed `guest` API, refreshed on every inspect.
let guestCompletions: Completion[] = [];

const apiCompletions = (ctx: CompletionContext): CompletionResult | null => {
  const word = ctx.matchBefore(/[\w.]+/);
  if (!word || (word.from === word.to && !ctx.explicit)) return null;
  return {
    from: word.from,
    options: [...API_COMPLETIONS, ...guestCompletions],
    validFor: /^[\w.]*$/,
  };
};

const editorTheme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--panel)" },
  ".cm-gutters": { backgroundColor: "var(--panel)" },
  ".cm-scroller": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: "13px",
  },
});

const editor = new EditorView({
  doc: scripts[EXAMPLES[0].id],
  extensions: [
    basicSetup,
    javascript(),
    javascriptLanguage.data.of({ autocomplete: apiCompletions }),
    editorTheme,
  ],
  parent: $("editor"),
});

const setEditor = (doc: string) => {
  editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: doc } });
};

// --- the AssemblyScript source editor (custom tab, default mode) ---

const DEFAULT_ASC = `// AssemblyScript compiles to a wasm guest. This one proves a private number x
// lies within a range whose bounds lo and hi are PUBLIC inputs (passed as
// Public(...) below). Only the 1/0 result is revealed, never x.
export function inRange(x: i32, lo: i32, hi: i32): i32 {
  return i32(x >= lo) * i32(x <= hi);
}
`;

const ascEditor = new EditorView({
  doc: DEFAULT_ASC,
  extensions: [
    basicSetup,
    javascript({ typescript: true }),
    editorTheme,
    EditorView.updateListener.of((u) => {
      if (u.docChanged) scheduleAscCompile();
    }),
  ],
  parent: $("asc-editor"),
});

// --- the guest module (built-in sha256, or a dropped .wasm) ---

const CUSTOM_WASM_CAP = 4 << 20; // 4 MB — guests are tens of KB

interface GuestModule {
  name: string;
  wasm: Uint8Array;
  builtin: boolean;
  exports: ExportInfo[] | null;
}

const exampleModules = new Map<string, GuestModule>(); // built-ins, by example id
const exampleForms = new Map<string, MountedForm>(); // mounted form panes, by id
let customModule: GuestModule | null = null;
let pendingInspect: GuestModule | null = null;
const inspectQueue: GuestModule[] = []; // one inspect in flight at a time
let lastCustomStarter = CUSTOM_SCRIPT; // the starter we last wrote (vs user edits)
let forceStarterRegen = false; // bypass the no-clobber gate once (on mode switch)

const currentModule = () =>
  currentTab === "custom" ? customModule : exampleModules.get(currentTab) ?? null;

const sha256Hex = async (bytes: Uint8Array) => {
  const digest = await crypto.subtle.digest("SHA-256", bytes as BufferSource);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
};

const renderModuleInfo = async (m: GuestModule) => {
  const hash = (await sha256Hex(m.wasm)).slice(0, 8);
  const n = m.exports ? m.exports.filter((e) => e.supported).length : 0;
  const note = m.exports ? ` · ${n} callable export(s)` : " · inspecting…";
  moduleInfo.textContent = `${m.name} · ${fmtBytes(m.wasm.length)} · sha-256 ${hash}…${note}`;
};

const refreshGuestCompletions = () => {
  const exports = currentModule()?.exports ?? [];
  guestCompletions = exports.map((e) => ({
    label: `guest.${e.name}`,
    type: "method",
    detail: `(${e.params.join(", ")})${e.results.length ? ` → ${e.results.join(",")}` : ""}`,
    info: e.supported
      ? "Calls this export interactively (returns a Promise)."
      : "Unsupported: the zk-vm calls only i32/i64 scalar functions.",
  }));
};

/// A starter orchestration listing a module's exports and calling the first
/// scalar one over a private argument — so a freshly compiled/dropped module
/// runs immediately.
const customStarter = (m: GuestModule) => {
  const supported = (m.exports ?? []).filter((e) => e.supported);
  const lines = supported
    .map((e) => `//   ${e.name}(${e.params.join(", ")})${e.results.length ? ` -> ${e.results.join(",")}` : ""}`)
    .join("\n");
  const first = supported[0];
  // First argument is the prover's secret; any further arguments are public.
  // Sensible placeholders: a secret of 42 within public bounds 18..65 (so the
  // default inRange(42, 18, 65) reads as "is my age in [18, 65]?" → yes).
  const publicVals = [18, 65, 100, 1, 7];
  const arg = (i: number) => (i === 0 ? "Private(42)" : `Public(${publicVals[(i - 1) % publicVals.length]})`);
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

// The prover worker inspects one module at a time; queue so concurrent requests
// (e.g. several built-ins loading at once) don't clobber `pendingInspect`.
const inspectModule = (m: GuestModule) => {
  inspectQueue.push(m);
  drainInspect();
};

const drainInspect = () => {
  if (pendingInspect || inspectQueue.length === 0) return;
  pendingInspect = inspectQueue.shift()!;
  workers.prover.postMessage({ type: "inspect", wasm: pendingInspect.wasm } satisfies PartyRequest);
};

const onExports = (exports: ExportInfo[]) => {
  const m = pendingInspect;
  pendingInspect = null;
  if (m) {
    m.exports = exports;
    if (m === customModule) {
      void renderModuleInfo(m);
      // Refresh the starter orchestration when the user hasn't edited it (so
      // recompiling AS as you type doesn't discard your script), or always on a
      // mode switch (so switching back to AS shows the AS starter, not the
      // previous WASM module's).
      const current = currentTab === "custom" ? editor.state.doc.toString() : scripts.custom;
      if (forceStarterRegen || current === lastCustomStarter) {
        scripts.custom = lastCustomStarter = customStarter(m);
        if (currentTab === "custom") setEditor(scripts.custom);
      }
      forceStarterRegen = false;
    }
    if (m === currentModule()) refreshGuestCompletions();
  }
  drainInspect();
};

const onExportsError = (message: string) => {
  pendingInspect = null;
  moduleInfo.textContent = `failed to parse module: ${message}`;
  guestCompletions = [];
  drainInspect();
};

/// Adopt freshly produced wasm (an upload or an AssemblyScript compile) as the
/// custom module: render its facts, inspect its exports, reveal the orchestration.
const setCustomWasm = (name: string, wasm: Uint8Array) => {
  customModule = { name, wasm, builtin: false, exports: null };
  void renderModuleInfo(customModule);
  inspectModule(customModule);
  applyVisibility();
};

const loadFile = async (file: File) => {
  if (file.size > CUSTOM_WASM_CAP) {
    moduleInfo.textContent = `${file.name} is too large (max ${fmtBytes(CUSTOM_WASM_CAP)})`;
    return;
  }
  setCustomWasm(file.name, new Uint8Array(await file.arrayBuffer()));
};

// --- AssemblyScript compilation (lazy: the compiler loads on first use) ---

type Asc = typeof import("assemblyscript/asc")["default"];
let asc: Asc | null = null;
let ascTimer: number | undefined;
let ascSeq = 0; // drops results from superseded compiles

const scheduleAscCompile = () => {
  window.clearTimeout(ascTimer);
  ascTimer = window.setTimeout(compileAsc, 400);
};

const compileAsc = async () => {
  const source = ascEditor.state.doc.toString();
  const seq = ++ascSeq;
  moduleInfo.textContent = "compiling AssemblyScript…";
  try {
    asc ??= (await import("assemblyscript/asc")).default;
    const { error, stderr, binary } = await asc.compileString(source, {
      optimizeLevel: 3,
      runtime: "stub",
    });
    if (seq !== ascSeq) return; // a newer edit already started compiling
    if (error || !binary) {
      customModule = null;
      forceStarterRegen = false;
      moduleInfo.textContent = `AssemblyScript error: ${error?.message ?? "compile failed"}\n${String(stderr)}`.trim();
      guestCompletions = [];
      applyVisibility();
      return;
    }
    setCustomWasm("assemblyscript.wasm", new Uint8Array(binary));
  } catch (e) {
    if (seq !== ascSeq) return;
    customModule = null;
    forceStarterRegen = false;
    moduleInfo.textContent = `AssemblyScript error: ${e instanceof Error ? e.message : String(e)}`;
    applyVisibility();
  }
};

const fetchWasm = async (url: string): Promise<Uint8Array> => {
  const res = await fetch(`${import.meta.env.BASE_URL}${url}`);
  if (!res.ok) {
    throw new Error(`guest wasm not found (HTTP ${res.status}) — run \`npm run build:guest\``);
  }
  return new Uint8Array(await res.arrayBuffer());
};

// --- guest tabs & the view-script toggle ---

// Mount one tab + form per example before the static `custom` entries; the
// first example starts active. This is the only place examples touch the DOM.
const customTabBtn = guestTabs.querySelector<HTMLButtonElement>('[data-tab="custom"]')!;
EXAMPLES.forEach((ex, i) => {
  guestTabs.insertBefore(exampleTab(ex, i === 0), customTabBtn);
  const form = exampleForm(ex);
  guestBody.insertBefore(form.el, customConfig);
  exampleForms.set(ex.id, form);
});

const isExample = (tab: string) => exampleForms.has(tab);

let currentTab = EXAMPLES[0].id;
let scriptShown = false;

/// Reflect `scriptShown` on every example's "view script" toggle.
const updateToggles = () => {
  for (const btn of guestBody.querySelectorAll<HTMLButtonElement>(".js-view-script")) {
    btn.textContent = scriptShown ? "hide script ▾" : "view script ▸";
  }
};

const applyVisibility = () => {
  for (const [id, form] of exampleForms) form.el.hidden = currentTab !== id;
  customConfig.hidden = currentTab !== "custom";
  // The editor shows for an example only when revealed, and for custom only once
  // a module has been provided (nothing to orchestrate without one).
  editorWrap.hidden = isExample(currentTab) ? !scriptShown : customModule === null;
};

const selectTab = (tab: string) => {
  scripts[currentTab] = editor.state.doc.toString();
  currentTab = tab;
  setEditor(scripts[tab]);
  if (isExample(tab)) {
    scriptShown = false;
    updateToggles();
  }
  applyVisibility();
  refreshGuestCompletions();
  // First visit to the custom tab in AS mode: compile the default source.
  if (tab === "custom" && customMode === "asc" && customModule === null) compileAsc();
};

initTabs(guestTabs, selectTab);
applyVisibility(); // show only the active example's form on load

// The example forms each carry a "view script" toggle (js-view-script).
guestBody.addEventListener("click", (ev) => {
  if (!(ev.target as HTMLElement).closest(".js-view-script")) return;
  scriptShown = !scriptShown;
  updateToggles();
  applyVisibility();
});

// --- custom tab: AssemblyScript vs upload mode ---

let customMode: "asc" | "upload" = "asc";

const applyCustomMode = () => {
  ascPane.hidden = customMode !== "asc";
  uploadPane.hidden = customMode !== "upload";
  for (const b of customModeEl.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
    b.classList.toggle("active", b.dataset.mode === customMode);
  }
  if (customMode === "asc") {
    forceStarterRegen = true; // show the AS starter, not the prior WASM module's
    compileAsc(); // the AS source is now the source of truth
  } else {
    customModule = null; // wait for an upload
    moduleInfo.textContent = "drop a .wasm module to begin";
    applyVisibility();
  }
};

customModeEl.addEventListener("click", (ev) => {
  const mode = (ev.target as HTMLElement).closest<HTMLButtonElement>("[data-mode]")?.dataset.mode;
  if ((mode !== "asc" && mode !== "upload") || mode === customMode) return;
  customMode = mode;
  applyCustomMode();
});

dropzone.addEventListener("click", () => wasmFileInput.click());
dropzone.addEventListener("dragover", (ev) => {
  ev.preventDefault();
  dropzone.classList.add("drag");
});
dropzone.addEventListener("dragleave", () => dropzone.classList.remove("drag"));
dropzone.addEventListener("drop", (ev) => {
  ev.preventDefault();
  dropzone.classList.remove("drag");
  const file = ev.dataTransfer?.files[0];
  if (file) void loadFile(file);
});
wasmFileInput.addEventListener("change", () => {
  const file = wasmFileInput.files?.[0];
  if (file) void loadFile(file);
  wasmFileInput.value = "";
});

// --- feature flag: the tamper button (default off) ---

cheatBtn.hidden = !FEATURES.cheat;
let cheatArmed = false;
cheatBtn.addEventListener("click", () => {
  cheatArmed = !cheatArmed;
  cheatBtn.classList.toggle("armed", cheatArmed);
  cheatBtn.textContent = cheatArmed ? "⚡ will tamper on the next run" : "⚡ tamper with a message";
});

// --- the run button ---

const SPINNER = '<span class="spinner" aria-hidden="true"></span>';

/// All run feedback lives on the button itself: a spinner while loading or
/// proving, a green ✓ on success, a red ✗ (with the message on hover) on
/// failure. `detail` is the hover title for the error state.
const setRunButton = (
  state: "loading" | "idle" | "running" | "done" | "error",
  detail = "",
) => {
  runBtn.disabled = state === "loading";
  runBtn.classList.toggle("done", state === "done");
  runBtn.classList.toggle("error", state === "error");
  if (state === "loading" || state === "running") {
    runBtn.innerHTML = SPINNER;
    runBtn.title = state === "running" ? "click to abort" : "loading…";
  } else {
    runBtn.textContent = state === "done" ? "✓" : state === "error" ? "✗" : "Prove";
    runBtn.title = state === "error" ? detail : "";
  }
};

// --- readiness ---

let readyCount = 0;
setRunButton("loading");

const markReady = () => {
  readyCount += 1;
  if (readyCount === 2) setRunButton("idle");
};

// --- one protocol run ---

/// Which relayed message a cheat corrupts: late enough to be meaningful,
/// early enough that every run reaches it.
const TAMPER_AT = 10;

type Dir = "prover→verifier" | "verifier→prover";

interface QueuedMsg {
  data: ArrayBuffer;
  to: MessagePort;
  dir: Dir;
}

interface Traffic {
  bytes: number;
}

interface RunState {
  pv: Traffic; // prover → verifier
  vp: Traffic; // verifier → prover
  count: number; // total relayed messages — internal, only to place a tamper
  start: number;
  results: Partial<Record<Role, string>>;
  ticker: number;
  queue: QueuedMsg[];
  pumping: boolean;
  tamper: boolean;
}
let run: RunState | null = null;

const showTraffic = (s: RunState) => {
  statPv.textContent = fmtBytes(s.pv.bytes);
  statVp.textContent = fmtBytes(s.vp.bytes);
};

const forward = (state: RunState, item: QueuedMsg) => {
  const t = item.dir === "prover→verifier" ? state.pv : state.vp;
  t.bytes += item.data.byteLength;
  state.count += 1;
  if (state.tamper && state.count === TAMPER_AT) {
    const view = new Uint8Array(item.data);
    view[Math.min(8, view.length - 1)] ^= 0x01;
  }
  item.to.postMessage(item.data, [item.data]);
};

const pump = (state: RunState) => {
  if (state.pumping) return;
  state.pumping = true;
  const step = () => {
    if (run !== state) return; // run ended
    const item = state.queue.shift();
    if (!item) {
      state.pumping = false;
      return;
    }
    forward(state, item);
    queueMicrotask(step);
  };
  step();
};

const endRun = () => {
  if (!run) return;
  clearInterval(run.ticker);
  run = null;
  if (cheatArmed) cheatBtn.click(); // disarm after one use
};

/// The protocol can't be interrupted mid-computation: kill both workers and
/// spawn fresh ones (they hold no state between runs).
const abortRun = () => {
  if (!run) return;
  endRun();
  for (const role of ["prover", "verifier"] as const) {
    workers[role].terminate();
    spawnWorker(role);
  }
  readyCount = 0;
  setRunButton("loading"); // a fresh pair of workers is loading
};

const finishRun = () => {
  if (!run) return;
  const { prover, verifier } = run.results;
  const elapsed = performance.now() - run.start;
  showTraffic(run);
  const ok = prover !== undefined && verifier !== undefined && prover === verifier;
  endRun();
  if (ok) {
    statTime.textContent = fmtMs(elapsed);
    resultEl.textContent = prover ?? "";
    resultBox.hidden = false;
    setRunButton("done");
  } else {
    setRunButton("error", "the parties disagree — the proof did not match");
  }
};

const failRun = (message: string) => {
  if (!run) return;
  endRun();
  setRunButton("error", `proof rejected — ${message}`);
};

const spawnWorker = (role: Role) => {
  const worker = new Worker(new URL("./party.worker.ts", import.meta.url), {
    type: "module",
  });
  worker.onmessage = (ev: MessageEvent<PartyResponse>) => {
    const msg = ev.data;
    switch (msg.type) {
      case "ready":
        markReady();
        break;
      case "done":
        if (!run) return;
        run.results[role] = msg.result;
        if (run.results.prover !== undefined && run.results.verifier !== undefined) {
          finishRun();
        }
        break;
      case "error":
        if (!run) {
          setRunButton("error", msg.message);
          break;
        }
        failRun(msg.message);
        break;
      case "exports": // only the prover worker is asked to inspect
        onExports(msg.exports);
        break;
      case "exports-error":
        onExportsError(msg.message);
        break;
    }
  };
  workers[role] = worker;
};

for (const role of ["prover", "verifier"] as const) spawnWorker(role);

initTooltips();

// Load and inspect each example's built-in module (keeps its example script).
for (const ex of EXAMPLES) {
  fetchWasm(ex.wasmUrl)
    .then((wasm) => {
      const m: GuestModule = { name: ex.moduleName, wasm, builtin: true, exports: null };
      exampleModules.set(ex.id, m);
      inspectModule(m);
    })
    .catch((e) => setRunButton("error", e instanceof Error ? e.message : String(e)));
}

runBtn.addEventListener("click", () => {
  if (run) {
    abortRun();
    return;
  }
  const m = currentModule();
  if (!m) {
    setRunButton("error", customMode === "asc" ? "fix the AssemblyScript first" : "drop a .wasm module first");
    return;
  }
  // Examples derive their inputs from their form; the custom tab's script
  // supplies its own (an empty private message is staged for compatibility).
  const ex = EXAMPLES.find((e) => e.id === currentTab);
  const { priv, pub } = ex
    ? ex.toInputs(exampleForms.get(ex.id)!.values())
    : { priv: { message: new Uint8Array(0) }, pub: { len: 0 } };

  const privBytes = Object.values(priv).reduce((n, a) => n + a.length, 0);
  if (privBytes > 128 * 1024) {
    setRunButton("error", "private input too long (max 128 KB)");
    return;
  }
  // The verifier sees private inputs blinded to their length (all zeros).
  const blinded = Object.fromEntries(
    Object.entries(priv).map(([k, v]) => [k, new Uint8Array(v.length)]),
  );

  const script = editor.state.doc.toString();

  setRunButton("running");
  resultBox.hidden = true;
  statTime.textContent = "—";
  statPv.textContent = "0 B";
  statVp.textContent = "0 B";

  // A fresh channel pair per run; the page relays the two workers' messages
  // through a queue, counting the traffic in each direction as it passes.
  const toProver = new MessageChannel();
  const toVerifier = new MessageChannel();
  const state: RunState = {
    pv: { bytes: 0 },
    vp: { bytes: 0 },
    count: 0,
    start: performance.now(),
    results: {},
    queue: [],
    pumping: false,
    tamper: cheatArmed,
    ticker: window.setInterval(() => {
      if (run === state) showTraffic(state);
    }, 100),
  };
  run = state;

  const relay = (from: MessagePort, to: MessagePort, dir: Dir) => {
    from.onmessage = (ev) => {
      state.queue.push({ data: ev.data as ArrayBuffer, to, dir });
      pump(state);
    };
  };
  // The prover holds toProver.port2, the verifier toVerifier.port2 — so a
  // message arriving on toProver.port1 came FROM the prover, and so on.
  relay(toProver.port1, toVerifier.port1, "prover→verifier");
  relay(toVerifier.port1, toProver.port1, "verifier→prover");

  const proverReq: PartyRequest = {
    type: "run", role: "prover", script, wasm: m.wasm, pub, priv, args: [],
  };
  const verifierReq: PartyRequest = {
    type: "run", role: "verifier", script, wasm: m.wasm, pub, priv: blinded, args: [],
  };
  workers.prover.postMessage(proverReq, [toProver.port2]);
  workers.verifier.postMessage(verifierReq, [toVerifier.port2]);
});
