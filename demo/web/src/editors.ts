// The two CodeMirror editors — the orchestration script editor and the
// AssemblyScript source editor — behind a tiny interface, so no other module
// imports CodeMirror. The static API completions, the per-module `guest`
// completions, and the shared theme all live here; the AS compile debounce does
// not (it's a compile concern, owned by the guest panel) — this module only
// emits `onAscChange` when the AS document changes.

import { EditorView, basicSetup } from "codemirror";
import { javascript, javascriptLanguage } from "@codemirror/lang-javascript";
import type {
  Completion,
  CompletionContext,
  CompletionResult,
} from "@codemirror/autocomplete";

export interface Editors {
  /// The orchestration editor's current document.
  getScript(): string;
  setScript(doc: string): void;
  /// The AssemblyScript source editor's current document.
  getAscSource(): string;
  /// Replace the per-module completions for the typed `guest` API.
  setGuestCompletions(completions: Completion[]): void;
}

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

const editorTheme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--panel)" },
  ".cm-gutters": { backgroundColor: "var(--panel)" },
  ".cm-scroller": {
    fontFamily: "ui-monospace, SFMono-Regular, Menlo, monospace",
    fontSize: "13px",
  },
});

export interface EditorsOpts {
  orchestrationParent: HTMLElement;
  ascParent: HTMLElement;
  initialScript: string;
  initialAsc: string;
  onAscChange(): void;
}

export const initEditors = (opts: EditorsOpts): Editors => {
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

  const editor = new EditorView({
    doc: opts.initialScript,
    extensions: [
      basicSetup,
      javascript(),
      javascriptLanguage.data.of({ autocomplete: apiCompletions }),
      editorTheme,
    ],
    parent: opts.orchestrationParent,
  });

  const ascEditor = new EditorView({
    doc: opts.initialAsc,
    extensions: [
      basicSetup,
      javascript({ typescript: true }),
      editorTheme,
      EditorView.updateListener.of((u) => {
        if (u.docChanged) opts.onAscChange();
      }),
    ],
    parent: opts.ascParent,
  });

  return {
    getScript: () => editor.state.doc.toString(),
    setScript: (doc) =>
      editor.dispatch({
        changes: { from: 0, to: editor.state.doc.length, insert: doc },
      }),
    getAscSource: () => ascEditor.state.doc.toString(),
    setGuestCompletions: (completions) => {
      guestCompletions = completions;
    },
  };
};
