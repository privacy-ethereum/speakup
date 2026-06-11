import { FEATURES } from "./config";
import type { PartyRequest, PartyResponse, Role } from "./party.worker";
import "./style.css";

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
const watRow = $("wat-row");
const watInput = $<HTMLTextAreaElement>("wat-input");
const cardInput = $<HTMLInputElement>("card-input");
const csvInput = $<HTMLTextAreaElement>("csv-input");
const csvRow = $("csv-row");
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

type ProgramKey =
  | "square"
  | "age"
  | "sha256"
  | "regex"
  | "luhn"
  | "csv"
  | "wat";

/// Today as the packed YYYYMMDD integer the age guest expects.
const todayPacked = () => {
  const d = new Date();
  return d.getFullYear() * 10000 + (d.getMonth() + 1) * 100 + d.getDate();
};

const utf8len = (s: string) => new TextEncoder().encode(s).length;
const hatch = (n: number) => `${"░".repeat(Math.max(1, Math.min(n, 24)))} (${n} bytes)`;

interface Program {
  source: string;
  input: HTMLInputElement | HTMLTextAreaElement;
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

const PROGRAMS: Record<ProgramKey, Program> = {
  square: {
    source: `fn compute(x: i32) -> i32 {
    reveal((x + 1) * (x + 1))
}`,
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
  wat: {
    source: `// compiled from the WAT editor
// (public — both parties compile
// the same program)`,
    input: xInput,
    inputLabel: "private input x",
    centerRow: watRow,
    requests() {
      const x = Number(xInput.value);
      if (!Number.isInteger(x)) return "x must be an integer";
      const source = watInput.value;
      if (!source.trim()) return "write a guest program";
      return {
        prover: { type: "run", role: "prover", program: "wat", source, x },
        verifier: { type: "run", role: "verifier", program: "wat", source, x: 0 },
      };
    },
    blind: () => "░░░░░░░░",
    proverStage: () => `staging private input x = ${xInput.value}`,
    render(result) {
      return { text: result, cls: "", log: `result: ${result}` };
    },
    secretName: "x",
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

const selectProgram = (key: ProgramKey) => {
  current = key;
  const p = PROGRAMS[key];
  for (const btn of tabs.querySelectorAll("button")) {
    btn.classList.toggle("active", btn.dataset.program === key);
  }
  for (const other of Object.values(PROGRAMS)) {
    other.input.hidden = true;
    if (other.centerRow) other.centerRow.hidden = true;
  }
  p.input.hidden = false;
  shaPresets.hidden = key !== "sha256";
  if (p.centerRow) p.centerRow.hidden = false;
  inputLabel.textContent = p.inputLabel;
  proverSource.textContent = p.source;
  verifierSource.textContent = p.source;
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

// --- feature flags ---

$("wat-tab").hidden = !FEATURES.watEditor;
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
