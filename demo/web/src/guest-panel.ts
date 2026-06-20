// The guest authoring panel: the guest tabs (built-in examples + custom), the
// custom tab's AssemblyScript-vs-upload modes, the dropzone, the per-example
// "view script" toggle, and the orchestration-script bookkeeping. Owns which tab
// is active, the custom module, the per-tab scripts, and the AssemblyScript
// compile debounce. It drives the editors (set/get script, refresh completions)
// and the inspector (inspect newly produced wasm) through the interfaces handed
// in at construction.

import { EXAMPLES, exampleForm, exampleTab, type MountedForm } from "./examples";
import { initTabs } from "./tabs";
import { fmtBytes } from "./dom";
import { mountVmEmbed } from "./vm-embed";
import { SCRIPT_TOGGLE } from "./copy";
import type { Editors } from "./editors";
import {
  CUSTOM_WASM_CAP,
  compileAscSource,
  customStarter,
  renderModuleInfo,
  type GuestModule,
  type Inspector,
} from "./guest-module";
import type { Completion } from "@codemirror/autocomplete";

const CUSTOM_SCRIPT = `// Drop a .wasm module above, then orchestrate it here against the typed
// \`guest\` API (autocomplete lists its exports once a module loads). Example:
//
//   const ptr = await guest.cabi_realloc(Public(0), Public(0), Public(1), Public(64));
//   vm.writePrivate(ptr, priv.message);
//   const out = await guest.hash(Public(ptr), Public(0));
//   return helpers.hex(vm.read(out, 32));

return "load a module and write your orchestration";
`;

/// The custom tab's default AssemblyScript source. Seeds the AS editor (passed to
/// `initEditors` by the page), so it lives here with the rest of the authoring
/// content.
export const DEFAULT_ASC = `// AssemblyScript compiles to a wasm guest. This one proves a private number x
// lies within a range whose bounds lo and hi are PUBLIC inputs (passed as
// Public(...) below). Only the 1/0 result is revealed, never x.
export function inRange(x: i32, lo: i32, hi: i32): i32 {
  return i32(x >= lo) * i32(x <= hi);
}
`;

export interface RunInputs {
  priv: Record<string, Uint8Array>;
  pub: Record<string, unknown>;
}

export interface GuestPanel {
  currentTab(): string;
  currentModule(): GuestModule | null;
  customMode(): "asc" | "upload";
  /// The AS source iff the custom tab is active in AssemblyScript mode (for the
  /// remote verifier to compile + hash-check); null otherwise.
  ascSourceIfCustomAsc(): string | null;
  /// The run inputs for the active tab (an example's `toInputs`, or the custom
  /// default).
  runInputs(): RunInputs;
  /// Adopt a freshly fetched built-in module and inspect it.
  registerExampleModule(id: string, m: GuestModule): void;
  /// An inspect resolved: refresh module-info, the starter script, completions.
  onModuleResolved(m: GuestModule): void;
  onInspectError(message: string): void;
  /// The AS source editor changed (debounce a recompile).
  onAscChange(): void;
}

export interface GuestPanelEls {
  guestTabs: HTMLElement;
  guestInfoTab: HTMLButtonElement;
  aboutPane: HTMLElement;
  aboutAnim: HTMLElement;
  guestBody: HTMLElement;
  customConfig: HTMLElement;
  customModeEl: HTMLElement;
  ascPane: HTMLElement;
  uploadPane: HTMLElement;
  dropzone: HTMLElement;
  wasmFileInput: HTMLInputElement;
  editorWrap: HTMLElement;
  moduleInfo: HTMLElement;
}

export const initGuestPanel = (opts: {
  els: GuestPanelEls;
  editors: Editors;
  inspector: Inspector;
  /// The active tab changed — lets the page react (e.g. hide the Prove button
  /// on the info tab, which has nothing to prove).
  onTabChange(tab: string): void;
}): GuestPanel => {
  const { els, editors, inspector } = opts;

  const exampleModules = new Map<string, GuestModule>(); // built-ins, by example id
  const exampleForms = new Map<string, MountedForm>(); // mounted form panes, by id
  let customModule: GuestModule | null = null;

  const scripts: Record<string, string> = {
    ...Object.fromEntries(EXAMPLES.map((ex) => [ex.id, ex.script])),
    custom: CUSTOM_SCRIPT,
  };
  let lastCustomStarter = CUSTOM_SCRIPT; // the starter we last wrote (vs user edits)
  let forceStarterRegen = false; // bypass the no-clobber gate once (on mode switch)

  let currentTab = "about"; // the info ("about") tab is the default view
  let scriptShown = false;
  let customMode: "asc" | "upload" = "asc";
  let aboutMounted = false; // the VM animation is mounted lazily on first view

  let ascTimer: number | undefined;
  let ascSeq = 0; // drops results from superseded compiles

  const currentModule = (): GuestModule | null =>
    currentTab === "custom" ? customModule : exampleModules.get(currentTab) ?? null;

  const isExample = (tab: string) => exampleForms.has(tab);

  const refreshGuestCompletions = () => {
    const exports = currentModule()?.exports ?? [];
    editors.setGuestCompletions(
      exports.map(
        (e): Completion => ({
          label: `guest.${e.name}`,
          type: "method",
          detail: `(${e.params.join(", ")})${e.results.length ? ` → ${e.results.join(",")}` : ""}`,
          info: e.supported
            ? "Calls this export interactively (returns a Promise)."
            : "Unsupported: the zk-vm calls only i32/i64 scalar functions.",
        }),
      ),
    );
  };

  // --- the custom module (a dropped .wasm or a compiled AS binary) ---

  /// Adopt freshly produced wasm: render its facts, inspect its exports, reveal
  /// the orchestration.
  const setCustomWasm = (name: string, wasm: Uint8Array) => {
    customModule = { name, wasm, builtin: false, exports: null };
    void renderModuleInfo(els.moduleInfo, customModule);
    inspector.inspect(customModule);
    applyVisibility();
  };

  const loadFile = async (file: File) => {
    if (file.size > CUSTOM_WASM_CAP) {
      els.moduleInfo.textContent = `${file.name} is too large (max ${fmtBytes(CUSTOM_WASM_CAP)})`;
      return;
    }
    setCustomWasm(file.name, new Uint8Array(await file.arrayBuffer()));
  };

  const scheduleAscCompile = () => {
    window.clearTimeout(ascTimer);
    ascTimer = window.setTimeout(compileAsc, 400);
  };

  const compileAsc = async () => {
    const seq = ++ascSeq;
    els.moduleInfo.textContent = "compiling AssemblyScript…";
    try {
      const wasm = await compileAscSource(editors.getAscSource());
      if (seq !== ascSeq) return; // a newer edit already started compiling
      setCustomWasm("assemblyscript.wasm", wasm);
    } catch (e) {
      if (seq !== ascSeq) return;
      customModule = null;
      forceStarterRegen = false;
      els.moduleInfo.textContent = `AssemblyScript error: ${e instanceof Error ? e.message : String(e)}`;
      editors.setGuestCompletions([]);
      applyVisibility();
    }
  };

  // --- tabs & the view-script toggle ---

  /// Reflect `scriptShown` on every example's "view script" toggle.
  const updateToggles = () => {
    for (const btn of els.guestBody.querySelectorAll<HTMLButtonElement>(".js-view-script")) {
      btn.textContent = scriptShown ? SCRIPT_TOGGLE.hide : SCRIPT_TOGGLE.show;
    }
  };

  const applyVisibility = () => {
    const about = currentTab === "about";
    els.aboutPane.hidden = !about;
    for (const [id, form] of exampleForms) form.el.hidden = currentTab !== id;
    els.customConfig.hidden = currentTab !== "custom";
    // The editor shows for an example only when revealed, and for custom only
    // once a module has been provided (nothing to orchestrate without one).
    els.editorWrap.hidden =
      about || (isExample(currentTab) ? !scriptShown : customModule === null);
  };

  const selectTab = (tab: string) => {
    const wasAbout = currentTab === "about";
    if (!wasAbout) scripts[currentTab] = editors.getScript();
    currentTab = tab;
    els.guestInfoTab.classList.toggle("active", tab === "about");
    // The "about" tab is a static info view — no script or module of its own.
    if (tab === "about") {
      applyVisibility();
      if (!aboutMounted) {
        // Mount the VM animation once the pane is first shown (it scales to the
        // now-visible pane's width).
        aboutMounted = true;
        mountVmEmbed(els.aboutAnim);
      }
      opts.onTabChange(tab); // hide the run button (also runs on the initial load)
      return;
    }
    editors.setScript(scripts[tab]);
    if (isExample(tab)) {
      scriptShown = false;
      updateToggles();
    }
    applyVisibility();
    refreshGuestCompletions();
    // First visit to the custom tab in AS mode: compile the default source.
    if (tab === "custom" && customMode === "asc" && customModule === null) compileAsc();
    if (wasAbout) opts.onTabChange(tab); // leaving about: show the run button again
  };

  // --- custom tab: AssemblyScript vs upload mode ---

  const applyCustomMode = () => {
    els.ascPane.hidden = customMode !== "asc";
    els.uploadPane.hidden = customMode !== "upload";
    for (const b of els.customModeEl.querySelectorAll<HTMLButtonElement>("[data-mode]")) {
      b.classList.toggle("active", b.dataset.mode === customMode);
    }
    if (customMode === "asc") {
      forceStarterRegen = true; // show the AS starter, not the prior WASM module's
      compileAsc(); // the AS source is now the source of truth
    } else {
      customModule = null; // wait for an upload
      els.moduleInfo.textContent = "drop a .wasm module to begin";
      applyVisibility();
    }
  };

  // --- mount examples and wire events ---

  const customTabBtn = els.guestTabs.querySelector<HTMLButtonElement>('[data-tab="custom"]')!;
  EXAMPLES.forEach((ex, i) => {
    els.guestTabs.insertBefore(exampleTab(ex, i === 0), customTabBtn);
    const form = exampleForm(ex);
    els.guestBody.insertBefore(form.el, els.customConfig);
    exampleForms.set(ex.id, form);
  });

  const guestTabs = initTabs(els.guestTabs, selectTab);
  // The "(i)" tab lives beside the strip (so it stays small), not inside it;
  // route its click through the same selection so it reads as another tab.
  els.guestInfoTab.addEventListener("click", () => guestTabs.select("about"));
  // Open on the info tab: clears the strip's initial active state, shows the
  // about pane, and mounts the animation.
  guestTabs.select("about");

  // The example forms each carry a "view script" toggle (js-view-script).
  els.guestBody.addEventListener("click", (ev) => {
    if (!(ev.target as HTMLElement).closest(".js-view-script")) return;
    scriptShown = !scriptShown;
    updateToggles();
    applyVisibility();
  });

  els.customModeEl.addEventListener("click", (ev) => {
    const mode = (ev.target as HTMLElement).closest<HTMLButtonElement>("[data-mode]")?.dataset.mode;
    if ((mode !== "asc" && mode !== "upload") || mode === customMode) return;
    customMode = mode;
    applyCustomMode();
  });

  els.dropzone.addEventListener("click", () => els.wasmFileInput.click());
  els.dropzone.addEventListener("dragover", (ev) => {
    ev.preventDefault();
    els.dropzone.classList.add("drag");
  });
  els.dropzone.addEventListener("dragleave", () => els.dropzone.classList.remove("drag"));
  els.dropzone.addEventListener("drop", (ev) => {
    ev.preventDefault();
    els.dropzone.classList.remove("drag");
    const file = ev.dataTransfer?.files[0];
    if (file) void loadFile(file);
  });
  els.wasmFileInput.addEventListener("change", () => {
    const file = els.wasmFileInput.files?.[0];
    if (file) void loadFile(file);
    els.wasmFileInput.value = "";
  });

  return {
    currentTab: () => currentTab,
    currentModule,
    customMode: () => customMode,
    ascSourceIfCustomAsc: () =>
      currentTab === "custom" && customMode === "asc" ? editors.getAscSource() : null,
    runInputs: () => {
      const ex = EXAMPLES.find((e) => e.id === currentTab);
      return ex
        ? ex.toInputs(exampleForms.get(ex.id)!.values())
        : { priv: { message: new Uint8Array(0) }, pub: { len: 0 } };
    },
    registerExampleModule: (id, m) => {
      exampleModules.set(id, m);
      inspector.inspect(m);
    },
    onModuleResolved: (m) => {
      if (m === customModule) {
        void renderModuleInfo(els.moduleInfo, m);
        // Refresh the starter orchestration when the user hasn't edited it (so
        // recompiling AS as you type doesn't discard your script), or always on
        // a mode switch (so switching back to AS shows the AS starter, not the
        // previous WASM module's).
        const current = currentTab === "custom" ? editors.getScript() : scripts.custom;
        if (forceStarterRegen || current === lastCustomStarter) {
          scripts.custom = lastCustomStarter = customStarter(m);
          if (currentTab === "custom") editors.setScript(scripts.custom);
        }
        forceStarterRegen = false;
      }
      if (m === currentModule()) refreshGuestCompletions();
    },
    onInspectError: (message) => {
      els.moduleInfo.textContent = `failed to parse module: ${message}`;
      editors.setGuestCompletions([]);
    },
    onAscChange: scheduleAscCompile,
  };
};
