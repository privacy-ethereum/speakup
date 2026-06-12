import { FEATURES } from "./config";
import type {
  EmbeddedProgram,
  ExportInfo,
  PartyRequest,
  PartyResponse,
  Role,
  TranscriptInfo,
} from "./party.worker";
import "./style.css";

// The real guest sources, embedded verbatim for the "view full source"
// modal — the same files build.rs compiles into the modules both parties run.
import squareSrc from "../../guests/square/src/lib.rs?raw";
import ageSrc from "../../guests/age/src/lib.rs?raw";
import sha256Src from "../../guests/sha256/src/lib.rs?raw";
import regexSrc from "../../guests/regex/src/lib.rs?raw";
import regexCoreSrc from "../../guests/regex-core/src/lib.rs?raw";
import luhnSrc from "../../guests/luhn/src/lib.rs?raw";
import csvSrc from "../../guests/csv/src/lib.rs?raw";
import transcriptSrc from "../../guests/transcript/src/lib.rs?raw";
import transcriptCoreSrc from "../../guests/transcript-core/src/lib.rs?raw";

// One worker per party: two isolated wasm memories. The prover's secrets
// physically cannot be in the verifier's address space. Spawned (and on
// abort, re-spawned) by `spawnWorker` below.
const workers = {} as Record<Role, Worker>;

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const runBtn = $<HTMLButtonElement>("run");
const tabs = $("program-tabs");
const xInput = $<HTMLInputElement>("x-input");
const dateInput = $<HTMLInputElement>("date-input");
const textInput = $<HTMLInputElement>("text-input");
const regexTextInput = $<HTMLInputElement>("regex-text-input");
const patternRow = $("pattern-row");
const patternInput = $<HTMLInputElement>("pattern-input");
const customRow = $("custom-row");
const dropzone = $("dropzone");
const wasmFileInput = $<HTMLInputElement>("wasm-file");
const customConfig = $("custom-config");
const funcSelect = $<HTMLSelectElement>("func-select");
const paramRows = $("param-rows");
const cardInput = $<HTMLInputElement>("card-input");
const csvInput = $<HTMLTextAreaElement>("csv-input");
const csvRow = $("csv-row");
const transcriptInput = $<HTMLTextAreaElement>("transcript-input");
const transcriptRow = $("transcript-row");
const transcriptClaim = $("transcript-claim");
const transcriptPath = $<HTMLSelectElement>("transcript-path");
const transcriptMode = $<HTMLSelectElement>("transcript-mode");
const transcriptExpect = $<HTMLInputElement>("transcript-expect");
const colInput = $<HTMLInputElement>("col-input");
const thresholdInput = $<HTMLInputElement>("threshold-input");
const inputLabel = $("input-label");
const proverSource = $("prover-source");
const verifierSource = $("verifier-source");
const proverLog = $("prover-log");
const verifierLog = $("verifier-log");
const blindCell = $("blind-cell");
const channel = $("channel");
const channelStatus = $("channel-status");
const resultBox = $("result-box");
const resultEl = $("result");
const delaySlider = $<HTMLInputElement>("delay-slider");
const delayValue = $("delay-value");
const cheatBtn = $<HTMLButtonElement>("cheat");
const shaPresets = $("sha-presets");
const proverWasmInfo = $("prover-wasm-info");
const verifierWasmInfo = $("verifier-wasm-info");
const fullSourceBtns = [
  $<HTMLButtonElement>("prover-full-source"),
  $<HTMLButtonElement>("verifier-full-source"),
];
const sourceModal = $("source-modal");
const sourceModalTitle = $("source-modal-title");
const sourceModalCode = $("source-modal-code");
const sourceModalClose = $<HTMLButtonElement>("source-modal-close");

type ProgramKey =
  | "square"
  | "age"
  | "sha256"
  | "regex"
  | "luhn"
  | "csv"
  | "transcript"
  | "ecdsa"
  | "custom";

/// Today as the packed YYYYMMDD integer the age guest expects.
const todayPacked = () => {
  const d = new Date();
  return d.getFullYear() * 10000 + (d.getMonth() + 1) * 100 + d.getDate();
};

const utf8len = (s: string) => new TextEncoder().encode(s).length;
const hatch = (n: number) => `${"░".repeat(Math.max(1, Math.min(n, 24)))} (${n} bytes)`;

interface Program {
  source: string;
  /// The real guest crate source, shown in the "view full source" modal.
  /// Absent for `custom`, where there is no source to show.
  fullSource?: { title: string; code: string };
  /// The embedded module to show wasm facts for. Absent for `custom`,
  /// whose facts come from the dropped file itself.
  module?: EmbeddedProgram;
  /// Prover-pane private input element. Absent for `custom`, whose
  /// arguments all live in the center panel.
  input?: HTMLInputElement | HTMLTextAreaElement;
  inputLabel: string;
  /// Center-column public input row, if the program has one.
  centerRow?: HTMLElement;
  /// Per-role requests, or an error message for invalid input.
  requests(): { prover: PartyRequest; verifier: PartyRequest } | string;
  /// What the verifier's blind view of the input looks like.
  blind(): string;
  proverStage(): string;
  /// Renders the revealed result as (display text, css class, log line).
  render(result: string): { text: string; cls: string; log: string };
  secretName: string;
}

// --- transcript fixture state (filled by the prover worker's answers) ---

let transcriptInfo: TranscriptInfo | null = null;
/// The flat public table for the current claim, and the (path, expect)
/// pair it was computed for (guards against running with a stale table).
let transcriptWords: Uint32Array | null = null;
let transcriptWordsKey = "";

/// `null` = disclose mode; a string = assert the field equals it.
const transcriptExpectValue = (): string | null =>
  transcriptMode.value === "assert" ? transcriptExpect.value : null;
const transcriptKey = () => {
  const e = transcriptExpectValue();
  return `${transcriptPath.value}\u0000${e === null ? "\u0000<disclose>" : e}`;
};

const PROGRAMS: Record<ProgramKey, Program> = {
  square: {
    source: `fn compute(x: i32) -> i32 {
    reveal((x + 1) * (x + 1))
}`,
    fullSource: { title: "guests/square/src/lib.rs", code: squareSrc },
    module: "square",
    input: xInput,
    inputLabel: "private input x",
    requests() {
      const x = Number(xInput.value);
      if (!Number.isInteger(x)) return "x must be an integer";
      return {
        prover: { type: "run", role: "prover", program: "square", x },
        verifier: { type: "run", role: "verifier", program: "square", x: 0 },
      };
    },
    blind: () => "░░░░░░░░",
    proverStage: () => `staging private input x = ${xInput.value}`,
    render(result) {
      return { text: result, cls: "", log: `result: ${result}` };
    },
    secretName: "x",
  },
  age: {
    source: `fn is_adult(today: i32) -> i32 {
    let date = load_birthdate(); // private
    reveal(age_flag(&date, today))
}`,
    fullSource: { title: "guests/age/src/lib.rs", code: ageSrc },
    module: "age",
    input: dateInput,
    inputLabel: "private birth date",
    requests() {
      const birthdate = dateInput.value;
      if (!/^\d{4}-\d{2}-\d{2}$/.test(birthdate)) return "pick a birth date";
      const today = todayPacked();
      return {
        prover: { type: "run", role: "prover", program: "age", birthdate, today },
        verifier: {
          type: "run",
          role: "verifier",
          program: "age",
          birthdate: "",
          today,
        },
      };
    },
    blind: () => "░░░░░░░░░░ (10 bytes)",
    proverStage: () => `staging private birth date ${dateInput.value}`,
    render(result) {
      const adult = result === "1";
      return {
        text: adult ? "✓ 18 or older" : "✗ not proven 18+",
        cls: adult ? "ok" : "no",
        log: adult
          ? "proved: 18 or older"
          : "not proven: under 18 (or invalid date)",
      };
    },
    secretName: "the birth date",
  },
  sha256: {
    source: `fn hash(msg: &[u8]) -> [u8; 32] {
    reveal_bytes(sha256(msg)) // digest only
}`,
    fullSource: { title: "guests/sha256/src/lib.rs", code: sha256Src },
    module: "sha256",
    input: textInput,
    inputLabel: "private message",
    requests() {
      const bytes = new TextEncoder().encode(textInput.value);
      if (bytes.length === 0) return "message must not be empty";
      if (bytes.length > 131072) return "message too long (max 128 KB)";
      return {
        prover: {
          type: "run",
          role: "prover",
          program: "sha256",
          message: bytes,
          msgLen: bytes.length,
        },
        verifier: {
          type: "run",
          role: "verifier",
          program: "sha256",
          message: new Uint8Array(),
          msgLen: bytes.length,
        },
      };
    },
    blind: () => hatch(utf8len(textInput.value)),
    proverStage: () => `staging private message (${utf8len(textInput.value)} bytes)`,
    render(result) {
      return {
        text: result,
        cls: "digest",
        log: `digest: ${result.slice(0, 16)}…`,
      };
    },
    secretName: "the message",
  },
  regex: {
    source: `fn matches(text: &[u8]) -> i32 {
    // DFA compiled from the public pattern,
    // evaluated obliviously over the
    // private text — branch-free
    reveal(dfa_matches(&TABLE, text))
}`,
    fullSource: {
      title: "guests/regex/src/lib.rs (+ regex-core)",
      code: `${regexSrc}\n// ───── guests/regex-core/src/lib.rs ─────\n\n${regexCoreSrc}`,
    },
    module: "regex",
    input: regexTextInput,
    inputLabel: "private test string",
    centerRow: patternRow,
    requests() {
      const pattern = patternInput.value;
      if (!pattern) return "enter a pattern";
      const text = regexTextInput.value;
      const len = utf8len(text);
      if (len === 0) return "test string must not be empty";
      if (len > 256) return "test string too long (max 256 bytes)";
      return {
        prover: { type: "run", role: "prover", program: "regex", pattern, text, textLen: len },
        verifier: {
          type: "run",
          role: "verifier",
          program: "regex",
          pattern,
          text: "",
          textLen: len,
        },
      };
    },
    blind: () => hatch(utf8len(regexTextInput.value)),
    proverStage: () =>
      `staging private test string (${utf8len(regexTextInput.value)} bytes)`,
    render(result) {
      const m = result === "1";
      return {
        text: m ? "✓ matches the pattern" : "✗ no match",
        cls: m ? "ok" : "no",
        log: m ? "proved: text matches the pattern" : "no match proven",
      };
    },
    secretName: "the test string",
  },
  luhn: {
    source: `fn check(len: i32) -> i32 {
    // Luhn checksum over the private
    // digits, branch-free
    reveal(luhn_valid(&NUMBER[..len]))
}`,
    fullSource: { title: "guests/luhn/src/lib.rs", code: luhnSrc },
    module: "luhn",
    input: cardInput,
    inputLabel: "private card number",
    requests() {
      const number = cardInput.value.replace(/[\s-]/g, "");
      if (!/^\d{12,19}$/.test(number)) return "enter 12-19 digits";
      return {
        prover: { type: "run", role: "prover", program: "luhn", number, numLen: number.length },
        verifier: {
          type: "run",
          role: "verifier",
          program: "luhn",
          number: "",
          numLen: number.length,
        },
      };
    },
    blind() {
      const n = cardInput.value.replace(/[\s-]/g, "").length;
      return `${"░".repeat(Math.max(1, Math.min(n, 24)))} (${n} digits)`;
    },
    proverStage() {
      const n = cardInput.value.replace(/[\s-]/g, "").length;
      return `staging private card number (${n} digits)`;
    },
    render(result) {
      const ok = result === "1";
      return {
        text: ok ? "✓ valid checksum" : "✗ invalid checksum",
        cls: ok ? "ok" : "no",
        log: ok ? "proved: the number passes Luhn" : "checksum does not pass",
      };
    },
    secretName: "the card number",
  },
  csv: {
    source: `fn mean_at_least(len, col, t) -> i32 {
    // the WHOLE document is private; the
    // guest parses it inside the VM:
    // tracks columns, builds numbers,
    // sums column \`col\` — branch-free
    reveal(parse_and_compare(len, col, t))
}`,
    fullSource: { title: "guests/csv/src/lib.rs", code: csvSrc },
    module: "csv",
    input: csvInput,
    inputLabel: "private CSV document",
    centerRow: csvRow,
    requests() {
      // Normalize: drop spaces and \r, ensure a trailing newline.
      const csv =
        csvInput.value.replace(/[ \r]/g, "").replace(/\n+$/, "") + "\n";
      const len = utf8len(csv);
      if (csv.trim() === "") return "paste a CSV document";
      if (len > 8192) return "CSV too large (max 8 KB)";
      if (!/^[0-9,\n]+$/.test(csv)) {
        return "cells must be plain numbers (digits, commas, newlines only)";
      }
      const col = Number(colInput.value);
      if (!Number.isInteger(col) || col < 0 || col > 15) {
        return "column must be 0..15";
      }
      const threshold = Number(thresholdInput.value);
      if (!Number.isInteger(threshold) || threshold < 0 || threshold > 99999) {
        return "threshold must be an integer 0..99999";
      }
      return {
        prover: { type: "run", role: "prover", program: "csv", csv, len, col, threshold },
        verifier: { type: "run", role: "verifier", program: "csv", csv: "", len, col, threshold },
      };
    },
    blind() {
      const len = utf8len(
        csvInput.value.replace(/[ \r]/g, "").replace(/\n+$/, "") + "\n",
      );
      return `${"░".repeat(Math.max(1, Math.min(len, 24)))} (${len} bytes)`;
    },
    proverStage() {
      const rows = csvInput.value.split("\n").filter((r) => r.trim()).length;
      return `staging private CSV (${rows} rows — row count stays private too)`;
    },
    render(result) {
      const ok = result === "1";
      return {
        text: ok ? "✓ column average ≥ threshold" : "✗ not proven",
        cls: ok ? "ok" : "no",
        log: ok
          ? "proved: well-formed CSV, column mean reaches the threshold"
          : "not proven: mean below threshold or malformed CSV",
      };
    },
    secretName: "the document (contents, row count, and sum)",
  },
  transcript: {
    source: `fn verify_transcript(...) -> i32 {
    // the public span table (parsed
    // OUTSIDE the VM by transcript-verify)
    // drives the walk; every private byte
    // is checked at its claimed position,
    // branch-free — incl. "field == expected"
    reveal(ok) // or reveal_bytes(value)
}`,
    fullSource: {
      title: "guests/transcript/src/lib.rs (+ transcript-core)",
      code: `${transcriptSrc}\n// ───── guests/transcript-core/src/lib.rs ─────\n\n${transcriptCoreSrc}`,
    },
    module: "transcript",
    input: transcriptInput,
    inputLabel: "private transcript — a captured HTTPS exchange",
    centerRow: transcriptRow,
    requests() {
      if (!transcriptInfo) return "the fixture is still loading — try again";
      const path = transcriptPath.value;
      if (!path) return "pick a JSON field";
      const expect = transcriptExpectValue();
      if (!transcriptWords || transcriptWordsKey !== transcriptKey()) {
        return "public inputs still computing — try again";
      }
      return {
        prover: {
          type: "run",
          role: "prover",
          program: "transcript",
          path,
          expect,
          words: new Uint32Array(),
        },
        verifier: {
          type: "run",
          role: "verifier",
          program: "transcript",
          path: "",
          expect,
          words: transcriptWords,
        },
      };
    },
    blind() {
      if (!transcriptInfo) return "—";
      const total = utf8len(transcriptInfo.sent) + utf8len(transcriptInfo.recv);
      return `░░░░░░░░░░░░ (${total} bytes — structure public, contents hidden)`;
    },
    proverStage() {
      const total = transcriptInfo
        ? utf8len(transcriptInfo.sent) + utf8len(transcriptInfo.recv)
        : 0;
      return `staging private transcript (${total} bytes)`;
    },
    render(result) {
      let parsed: { ok: number; value: string };
      try {
        parsed = JSON.parse(result);
      } catch {
        return { text: result, cls: "", log: `result: ${result}` };
      }
      const path = transcriptPath.value;
      const expect = transcriptExpectValue();
      if (parsed.ok === 1) {
        return expect === null
          ? {
              text: `✓ ${path} = ${JSON.stringify(parsed.value)}`,
              cls: "ok",
              log: `proved the exchange and disclosed ${path} = ${JSON.stringify(parsed.value)}`,
            }
          : {
              text: `✓ ${path} = ${JSON.stringify(expect)} — proven`,
              cls: "ok",
              log: `proved the exchange and that ${path} = ${JSON.stringify(expect)} — the value itself was never sent`,
            };
      }
      return {
        text: "✗ not proven",
        cls: "no",
        log:
          expect === null
            ? "the transcript does not match the claimed parse"
            : `not proven: ${path} ≠ ${JSON.stringify(expect)} (or the parse is invalid)`,
      };
    },
    secretName: "the transcript (every header and field other than the claims)",
  },
  // Placeholder: no guest exists yet. The tab shows what's planned and the
  // Run button explains itself; everything else (guest crate, bindings,
  // worker plumbing) lands with the implementation.
  ecdsa: {
    source: `// coming soon
//
// fn verify(msg, sig, pubkey) -> i32 {
//     // verify an ECDSA signature
//     // INSIDE the zk-vm: prove a
//     // private message carries a
//     // valid signature, revealing
//     // neither message nor signature
//     reveal(ecdsa_verify(...))
// }`,
    inputLabel: "private message & signature",
    requests() {
      return "ecdsa is not implemented yet — coming soon";
    },
    blind: () => "—",
    proverStage: () => "",
    render(result) {
      return { text: result, cls: "", log: `result: ${result}` };
    },
    secretName: "the message and signature",
  },
  custom: {
    source: `// drop a compiled guest module
// to inspect its exports`,
    inputLabel: "private input",
    centerRow: customRow,
    requests() {
      if (!customWasm) return "drop a .wasm guest first";
      const exp = selectedExport();
      if (!exp || !exp.supported) return "pick a supported function";
      const n = exp.params.length;
      const vis = new Uint8Array(n);
      const values = new BigInt64Array(n);
      const blindValues = new BigInt64Array(n);
      for (let i = 0; i < n; i++) {
        let v: bigint;
        try {
          v = BigInt(paramValue(i).value.trim());
        } catch {
          return `arg${i} must be an integer`;
        }
        const lim = exp.params[i] === "i32" ? 31n : 63n;
        if (v < -(2n ** lim) || v >= 2n ** lim) {
          return `arg${i} is out of ${exp.params[i]} range`;
        }
        const priv = paramPrivate(i).checked;
        vis[i] = priv ? 1 : 0;
        values[i] = v;
        blindValues[i] = priv ? 0n : v; // the verifier never sees private values
      }
      const base = {
        type: "run" as const,
        program: "custom" as const,
        wasm: customWasm.bytes,
        func: exp.name,
        vis,
      };
      return {
        prover: { ...base, role: "prover" as const, values },
        verifier: { ...base, role: "verifier" as const, values: blindValues },
      };
    },
    blind() {
      const exp = selectedExport();
      if (!customWasm || !exp) return "—";
      return exp.params
        .map((ty, i) =>
          paramPrivate(i).checked ? `░░░░ (${ty})` : `${paramValue(i).value} (public)`,
        )
        .join(" · ");
    },
    proverStage() {
      const exp = selectedExport();
      const n = exp ? exp.params.filter((_, i) => paramPrivate(i).checked).length : 0;
      return `staging ${n} private argument${n === 1 ? "" : "s"}`;
    },
    render(result) {
      return { text: result, cls: "", log: `result: ${result}` };
    },
    secretName: "the private input",
  },
};

let current: ProgramKey = "square";

const log = (el: HTMLElement, text: string, cls = "") => {
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  line.textContent = text;
  el.appendChild(line);
  el.scrollTop = el.scrollHeight;
};

const clearLogs = () => {
  proverLog.replaceChildren();
  verifierLog.replaceChildren();
};

const fmtBytes = (n: number) =>
  n < 1024 ? `${n} B` : n < 1 << 20 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`;

/// Asks both parties for facts about their embedded module; each pane shows
/// its own party's answer when it arrives (see the `guest_info` case below).
const requestGuestInfo = (p: Program) => {
  proverWasmInfo.textContent = "";
  verifierWasmInfo.textContent = "";
  if (!p.module) return;
  const req: PartyRequest = { type: "guest_info", program: p.module };
  workers.prover.postMessage(req);
  workers.verifier.postMessage(req);
};

const selectProgram = (key: ProgramKey) => {
  current = key;
  const p = PROGRAMS[key];
  for (const btn of tabs.querySelectorAll("button")) {
    btn.classList.toggle("active", btn.dataset.program === key);
  }
  for (const other of Object.values(PROGRAMS)) {
    if (other.input) other.input.hidden = true;
    if (other.centerRow) other.centerRow.hidden = true;
  }
  if (p.input) p.input.hidden = false;
  // No prover-pane input for custom: its arguments live in the center panel.
  (inputLabel.parentElement as HTMLElement).hidden = !p.input;
  shaPresets.hidden = key !== "sha256";
  if (p.centerRow) p.centerRow.hidden = false;
  inputLabel.textContent = p.inputLabel;
  proverSource.textContent = p.source;
  verifierSource.textContent = p.source;
  // The fixture is parsed once, lazily, by the prover's worker.
  if (key === "transcript" && !transcriptInfo) {
    workers.prover.postMessage({ type: "transcript_info" } satisfies PartyRequest);
  }
  for (const btn of fullSourceBtns) btn.hidden = !p.fullSource;
  requestGuestInfo(p);
  if (key === "custom" && customInfoLine) {
    proverWasmInfo.textContent = customInfoLine;
    verifierWasmInfo.textContent = customInfoLine;
  }
  blindCell.textContent = p.blind();
  resultBox.hidden = true;
  clearLogs();
};

tabs.addEventListener("click", (ev) => {
  const btn = (ev.target as HTMLElement).closest("button");
  if (btn?.dataset.program) selectProgram(btn.dataset.program as ProgramKey);
});

// Preset messages for the sha-256 program: deterministic filler of an exact
// size, so runs are reproducible and the cost scaling is visible.
shaPresets.addEventListener("click", (ev) => {
  const btn = (ev.target as HTMLElement).closest("button");
  if (!btn?.dataset.size) return;
  const size = Number(btn.dataset.size);
  textInput.value = "speakup demo data ".repeat(Math.ceil(size / 18)).slice(0, size);
  blindCell.textContent = PROGRAMS.sha256.blind();
});

// --- custom module (dropped .wasm) ---

const CUSTOM_WASM_CAP = 1 << 20; // 1 MB — guests are tens of KB

let customWasm: { name: string; bytes: Uint8Array } | null = null;
let customExports: ExportInfo[] = [];
/// The wasm-info line for the loaded file, restored when re-entering the tab.
let customInfoLine = "";

const selectedExport = () => customExports.find((e) => e.name === funcSelect.value);
const paramValue = (i: number) => $<HTMLInputElement>(`param-value-${i}`);
const paramPrivate = (i: number) => $<HTMLInputElement>(`param-priv-${i}`);

const fmtSig = (e: ExportInfo) =>
  `${e.name}(${e.params.join(", ")})${e.results.length ? ` -> ${e.results.join(", ")}` : ""}`;

/// One row per argument of the selected function: a value input and a
/// private toggle. The first argument defaults to private.
const buildParamRows = () => {
  paramRows.replaceChildren();
  const exp = selectedExport();
  if (!exp) return;
  exp.params.forEach((ty, i) => {
    const row = document.createElement("div");
    row.className = "param-row";
    const name = document.createElement("span");
    name.className = "param-name";
    name.textContent = `arg${i} (${ty})`;
    const value = document.createElement("input");
    value.type = "text";
    value.inputMode = "numeric";
    value.value = "0";
    value.id = `param-value-${i}`;
    value.addEventListener("input", () => {
      blindCell.textContent = PROGRAMS.custom.blind();
    });
    const privLabel = document.createElement("label");
    privLabel.className = "param-priv";
    const priv = document.createElement("input");
    priv.type = "checkbox";
    priv.checked = i === 0;
    priv.id = `param-priv-${i}`;
    priv.addEventListener("change", () => {
      blindCell.textContent = PROGRAMS.custom.blind();
    });
    privLabel.append(priv, " private");
    row.append(name, value, privLabel);
    paramRows.append(row);
  });
  blindCell.textContent = PROGRAMS.custom.blind();
};

/// The prover worker's answer to an `inspect` request.
const onExports = (msg: { exports?: ExportInfo[]; error?: string }) => {
  if (!customWasm) return;
  if (msg.error || !msg.exports) {
    const reason = (msg.error ?? "no exports").replace(/\s+/g, " ");
    dropzone.textContent = `not a valid Speakup guest: ${
      reason.length > 90 ? `${reason.slice(0, 90)}…` : reason
    }`;
    customWasm = null;
    customInfoLine = "";
    return;
  }
  customExports = msg.exports;
  const supported = customExports.filter((e) => e.supported);
  funcSelect.replaceChildren();
  for (const e of customExports) {
    const opt = document.createElement("option");
    opt.value = e.name;
    opt.textContent = fmtSig(e) + (e.supported ? "" : " — unsupported");
    opt.disabled = !e.supported;
    funcSelect.append(opt);
  }
  if (!supported.length) {
    dropzone.textContent = `${customWasm.name}: no callable exports (i32/i64 scalars only)`;
    return;
  }
  funcSelect.value = supported[0].name;
  buildParamRows();
  customConfig.hidden = false;
  dropzone.textContent = `${customWasm.name} · ${fmtBytes(customWasm.bytes.length)} ✓ — drop another to replace`;
  const src = supported.map((e) => `fn ${fmtSig(e)}`).join("\n");
  proverSource.textContent = src;
  verifierSource.textContent = src;
};

const loadWasmFile = async (file: File) => {
  if (file.size > CUSTOM_WASM_CAP) {
    dropzone.textContent = `${file.name} is too big (max ${fmtBytes(CUSTOM_WASM_CAP)})`;
    return;
  }
  const buffer = await file.arrayBuffer();
  customWasm = { name: file.name, bytes: new Uint8Array(buffer) };
  customExports = [];
  customConfig.hidden = true;
  dropzone.textContent = `${file.name} · ${fmtBytes(file.size)} — inspecting…`;
  const digest = await crypto.subtle.digest("SHA-256", buffer);
  const sha256 = [...new Uint8Array(digest)]
    .map((b) => b.toString(16).padStart(2, "0"))
    .join("");
  customInfoLine = `wasm ${fmtBytes(file.size)} · sha-256 ${sha256.slice(0, 8)}…`;
  proverWasmInfo.textContent = customInfoLine;
  verifierWasmInfo.textContent = customInfoLine;
  workers.prover.postMessage({ type: "inspect", wasm: customWasm.bytes } satisfies PartyRequest);
};

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
  if (file) void loadWasmFile(file);
});
wasmFileInput.addEventListener("change", () => {
  const file = wasmFileInput.files?.[0];
  if (file) void loadWasmFile(file);
  wasmFileInput.value = "";
});
funcSelect.addEventListener("change", buildParamRows);

// --- transcript fixture plumbing ---

/// Truncates a value preview for the dropdown.
const preview = (v: string) => (v.length > 28 ? `${v.slice(0, 28)}…` : v);

/// Asks the prover's worker for the flat public table of the selected
/// claim (path + mode + expected value).
const requestTranscriptWords = () => {
  const path = transcriptPath.value;
  if (!path) return;
  transcriptWords = null;
  workers.prover.postMessage({
    type: "transcript_public",
    path,
    expect: transcriptExpectValue(),
  } satisfies PartyRequest);
};

/// Prefills the expected-value input with the selected field's actual
/// value (edit it to watch the proof legitimately fail).
const prefillTranscriptExpect = () => {
  const v = transcriptInfo?.paths.find((p) => p.path === transcriptPath.value);
  if (v) transcriptExpect.value = v.value;
  transcriptExpect.hidden = transcriptMode.value !== "assert";
};

/// The prover worker's answer to a `transcript_info` request: fill the
/// fixture pane, the claim line, and the path dropdown.
const onTranscriptInfo = (msg: { info?: TranscriptInfo; error?: string }) => {
  if (!msg.info) {
    transcriptClaim.textContent = `fixture failed to parse: ${msg.error ?? "unknown"}`;
    return;
  }
  transcriptInfo = msg.info;
  const info = msg.info;
  transcriptInput.value = `${info.sent}\n${info.recv}`;
  transcriptClaim.textContent =
    `${info.method} ${info.target} → ${info.host} · response status ${info.status}` +
    (info.reqBody ? " · request body hidden" : "");
  transcriptPath.replaceChildren();
  for (const p of info.paths) {
    const opt = document.createElement("option");
    opt.value = p.path;
    opt.textContent = `${p.path} = ${JSON.stringify(preview(p.value))}`;
    transcriptPath.append(opt);
  }
  const def = info.paths.find((p) => p.path === "id") ?? info.paths[0];
  if (def) transcriptPath.value = def.path;
  prefillTranscriptExpect();
  requestTranscriptWords();
  if (current === "transcript") blindCell.textContent = PROGRAMS.transcript.blind();
};

const onTranscriptPublic = (msg: {
  path: string;
  expect: string | null;
  words?: Uint32Array;
  error?: string;
}) => {
  // Stale answer for a claim the user has since changed.
  if (msg.path !== transcriptPath.value || msg.expect !== transcriptExpectValue()) return;
  if (!msg.words) {
    log(proverLog, `public inputs failed: ${msg.error ?? "unknown"}`, "err");
    return;
  }
  transcriptWords = msg.words;
  transcriptWordsKey = transcriptKey();
};

transcriptPath.addEventListener("change", () => {
  prefillTranscriptExpect();
  requestTranscriptWords();
});
transcriptMode.addEventListener("change", () => {
  transcriptExpect.hidden = transcriptMode.value !== "assert";
  requestTranscriptWords();
});
transcriptExpect.addEventListener("input", requestTranscriptWords);

// --- full-source modal ---

for (const btn of fullSourceBtns) {
  btn.addEventListener("click", () => {
    const f = PROGRAMS[current].fullSource;
    if (!f) return;
    sourceModalTitle.textContent = f.title;
    sourceModalCode.textContent = f.code;
    sourceModal.hidden = false;
  });
}
const closeSourceModal = () => {
  sourceModal.hidden = true;
};
sourceModalClose.addEventListener("click", closeSourceModal);
sourceModal.addEventListener("click", (ev) => {
  if (ev.target === sourceModal) closeSourceModal();
});
document.addEventListener("keydown", (ev) => {
  if (ev.key === "Escape") closeSourceModal();
});

// --- feature flags ---

cheatBtn.hidden = !FEATURES.cheat;

delaySlider.addEventListener("input", () => {
  delayValue.textContent = `${delaySlider.value} ms`;
});

let cheatArmed = false;
cheatBtn.addEventListener("click", () => {
  cheatArmed = !cheatArmed;
  cheatBtn.classList.toggle("armed", cheatArmed);
  cheatBtn.textContent = cheatArmed
    ? "⚡ will tamper on the next run"
    : "⚡ tamper with a message";
});

// --- readiness ---

let readyCount = 0;
runBtn.disabled = true;
channelStatus.textContent = "loading wasm…";

const markReady = () => {
  readyCount += 1;
  if (readyCount === 2) {
    runBtn.disabled = false;
    channelStatus.textContent = "idle";
    // Freshly spawned workers (initial load or post-abort) haven't been
    // asked about the current module yet.
    requestGuestInfo(PROGRAMS[current]);
  }
};

// --- one protocol run ---

/// Which relayed message a cheat corrupts: late enough to be past OT setup,
/// early enough that every program reaches it.
const TAMPER_AT = 10;

/// Message direction; the arrows match the panes' layout (prover left,
/// verifier right).
type Dir = "prover→verifier" | "verifier→prover";

interface QueuedMsg {
  data: ArrayBuffer;
  to: MessagePort;
  dir: Dir;
}

interface RunState {
  msgs: number;
  bytes: Record<Dir, number>;
  start: number;
  results: Partial<Record<Role, string>>;
  ticker: number;
  queue: QueuedMsg[];
  pumping: boolean;
  tamper: boolean;
}
let run: RunState | null = null;

const fmtTraffic = (s: RunState) =>
  `${s.msgs} msgs · →${fmtBytes(s.bytes["prover→verifier"])} · ←${fmtBytes(s.bytes["verifier→prover"])}`;

/// Forwards one queued message, tampering if this run is the cheating one.
const forward = (state: RunState, item: QueuedMsg) => {
  state.msgs += 1;
  state.bytes[item.dir] += item.data.byteLength;
  if (state.tamper && state.msgs === TAMPER_AT) {
    const view = new Uint8Array(item.data);
    view[Math.min(8, view.length - 1)] ^= 0x01;
    const note = `⚡ the relay tampered with message #${state.msgs} (${item.dir}) — one bit flipped`;
    log(proverLog, note, "warn");
    log(verifierLog, note, "warn");
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
    const delay = Number(delaySlider.value);
    if (delay > 0) setTimeout(step, delay);
    else queueMicrotask(step);
  };
  step();
};

/// The run button doubles as the abort button while a run is active.
const setRunButton = (running: boolean) => {
  runBtn.textContent = running ? "Abort" : "Run in zero-knowledge";
  runBtn.classList.toggle("abort", running);
};

const endRun = () => {
  if (!run) return;
  clearInterval(run.ticker);
  channel.classList.remove("active");
  run = null;
  setRunButton(false);
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
  runBtn.disabled = true;
  channelStatus.textContent = "aborted — reloading wasm…";
  log(proverLog, "run aborted", "warn");
  log(verifierLog, "run aborted", "warn");
};

const finishRun = () => {
  if (!run) return;
  const p = PROGRAMS[current];
  const { prover, verifier } = run.results;
  const elapsed = performance.now() - run.start;
  const traffic = fmtTraffic(run);
  if (prover !== undefined && verifier !== undefined && prover === verifier) {
    const delayed = Number(delaySlider.value) > 0 ? " (with simulated latency)" : "";
    channelStatus.textContent = `proof complete in ${elapsed.toFixed(0)} ms${delayed}\n${traffic}`;
    const r = p.render(prover);
    resultEl.textContent = r.text;
    resultEl.className = `result ${r.cls}`;
    resultBox.hidden = false;
    log(proverLog, `revealed — ${r.log}`, "ok");
    log(verifierLog, `proof checked ✓ — ${r.log}`, "ok");
    log(verifierLog, `${p.secretName} was never disclosed`, "muted");
  } else {
    channelStatus.textContent = "failed";
    log(verifierLog, `parties disagree: ${prover} vs ${verifier}`, "err");
  }
  endRun();
};

const failRun = (role: Role, message: string) => {
  if (!run) return;
  channelStatus.textContent = "proof rejected ✗";
  log(role === "prover" ? proverLog : verifierLog, message, "err");
  log(verifierLog, "the proof did not verify — rejected", "err");
  endRun();
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
        log(
          role === "prover" ? proverLog : verifierLog,
          `${role} finished in ${msg.ms.toFixed(0)} ms`,
        );
        if (run.results.prover !== undefined && run.results.verifier !== undefined) {
          finishRun();
        }
        break;
      case "error":
        failRun(role, msg.message);
        break;
      case "guest_info":
        if (msg.program !== PROGRAMS[current].module) return; // stale
        (role === "prover" ? proverWasmInfo : verifierWasmInfo).textContent =
          `wasm ${fmtBytes(msg.size)} · sha-256 ${msg.sha256.slice(0, 8)}…`;
        (role === "prover" ? proverWasmInfo : verifierWasmInfo).title =
          `sha-256 ${msg.sha256}`;
        break;
      case "exports":
        onExports(msg);
        break;
      case "transcript_info":
        onTranscriptInfo(msg);
        break;
      case "transcript_public":
        onTranscriptPublic(msg);
        break;
    }
  };
  workers[role] = worker;
};

for (const role of ["prover", "verifier"] as const) spawnWorker(role);

runBtn.addEventListener("click", () => {
  if (run) {
    abortRun();
    return;
  }
  const p = PROGRAMS[current];
  const reqs = p.requests();
  if (typeof reqs === "string") {
    log(proverLog, reqs, "err");
    return;
  }
  setRunButton(true);
  resultBox.hidden = true;
  clearLogs();
  blindCell.textContent = p.blind();
  channel.classList.add("active");

  // A fresh channel pair per run; the page relays prover <-> verifier
  // through a queue, counting (and optionally delaying or tampering with)
  // the traffic as it passes through.
  const toProver = new MessageChannel();
  const toVerifier = new MessageChannel();
  const state: RunState = {
    msgs: 0,
    bytes: { "prover→verifier": 0, "verifier→prover": 0 },
    start: performance.now(),
    results: {},
    queue: [],
    pumping: false,
    tamper: cheatArmed,
    ticker: window.setInterval(() => {
      if (run === state) {
        channelStatus.textContent = `exchanging…\n${fmtTraffic(state)}`;
      }
    }, 100),
  };
  run = state;

  const relay = (from: MessagePort, to: MessagePort, dir: Dir) => {
    from.onmessage = (ev) => {
      state.queue.push({ data: ev.data as ArrayBuffer, to, dir });
      pump(state);
    };
  };
  relay(toProver.port1, toVerifier.port1, "prover→verifier");
  relay(toVerifier.port1, toProver.port1, "verifier→prover");

  channelStatus.textContent = "exchanging…";
  log(proverLog, p.proverStage());
  log(verifierLog, "blind slot allocated — bytes unknown");
  log(proverLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(verifierLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(proverLog, "executing on Speakup");
  log(verifierLog, "verifying every instruction…");
  if (state.tamper) {
    log(verifierLog, "⚡ cheat armed: the relay will corrupt one message", "warn");
  }
  workers.prover.postMessage(reqs.prover, [toProver.port2]);
  workers.verifier.postMessage(reqs.verifier, [toVerifier.port2]);
});

selectProgram("square");
