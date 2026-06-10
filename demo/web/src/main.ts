import type { PartyRequest, PartyResponse, Role } from "./party.worker";
import "./style.css";

// One worker per party: two isolated wasm memories. The prover's secrets
// physically cannot be in the verifier's address space.
const workers: Record<Role, Worker> = {
  prover: new Worker(new URL("./party.worker.ts", import.meta.url), {
    type: "module",
  }),
  verifier: new Worker(new URL("./party.worker.ts", import.meta.url), {
    type: "module",
  }),
};

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const runBtn = $<HTMLButtonElement>("run");
const tabs = $("program-tabs");
const xInput = $<HTMLInputElement>("x-input");
const dateInput = $<HTMLInputElement>("date-input");
const textInput = $<HTMLInputElement>("text-input");
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

type ProgramKey = "square" | "age" | "sha256";

/// Today as the packed YYYYMMDD integer the age guest expects.
const todayPacked = () => {
  const d = new Date();
  return d.getFullYear() * 10000 + (d.getMonth() + 1) * 100 + d.getDate();
};

interface Program {
  source: string;
  input: HTMLInputElement;
  inputLabel: string;
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
      if (bytes.length > 4096) return "message too long (max 4096 bytes)";
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
    blind() {
      const n = new TextEncoder().encode(textInput.value).length;
      return `${"░".repeat(Math.min(n, 24))} (${n} bytes)`;
    },
    proverStage() {
      const n = new TextEncoder().encode(textInput.value).length;
      return `staging private message (${n} bytes)`;
    },
    render(result) {
      return {
        text: result,
        cls: "digest",
        log: `digest: ${result.slice(0, 16)}…`,
      };
    },
    secretName: "the message",
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
  for (const other of Object.values(PROGRAMS)) other.input.hidden = true;
  p.input.hidden = false;
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

interface RunState {
  msgs: number;
  bytes: number;
  start: number;
  results: Partial<Record<Role, string>>;
  ticker: number;
}
let run: RunState | null = null;

const finishRun = () => {
  if (!run) return;
  const p = PROGRAMS[current];
  const { prover, verifier } = run.results;
  channel.classList.remove("active");
  clearInterval(run.ticker);
  const elapsed = performance.now() - run.start;
  const traffic = `${run.msgs} msgs · ${fmtBytes(run.bytes)}`;
  if (prover !== undefined && verifier !== undefined && prover === verifier) {
    channelStatus.textContent = `proof complete in ${elapsed.toFixed(0)} ms — ${traffic}`;
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
  run = null;
  runBtn.disabled = false;
};

const failRun = (role: Role, message: string) => {
  if (!run) return;
  clearInterval(run.ticker);
  channel.classList.remove("active");
  channelStatus.textContent = "failed";
  log(role === "prover" ? proverLog : verifierLog, message, "err");
  run = null;
  runBtn.disabled = false;
};

for (const role of ["prover", "verifier"] as const) {
  workers[role].onmessage = (ev: MessageEvent<PartyResponse>) => {
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
}

runBtn.addEventListener("click", () => {
  if (run) return;
  const p = PROGRAMS[current];
  const reqs = p.requests();
  if (typeof reqs === "string") {
    log(proverLog, reqs, "err");
    return;
  }
  runBtn.disabled = true;
  resultBox.hidden = true;
  clearLogs();
  blindCell.textContent = p.blind();
  channel.classList.add("active");

  // A fresh channel pair per run; the page relays prover <-> verifier and
  // counts the traffic as it passes through.
  const toProver = new MessageChannel();
  const toVerifier = new MessageChannel();
  const state: RunState = {
    msgs: 0,
    bytes: 0,
    start: performance.now(),
    results: {},
    ticker: window.setInterval(() => {
      if (run === state) {
        channelStatus.textContent = `exchanging… ${state.msgs} msgs · ${fmtBytes(state.bytes)}`;
      }
    }, 100),
  };
  run = state;

  const relay = (from: MessagePort, to: MessagePort) => {
    from.onmessage = (ev) => {
      const data = ev.data as ArrayBuffer;
      state.msgs += 1;
      state.bytes += data.byteLength;
      to.postMessage(data, [data]);
    };
  };
  relay(toProver.port1, toVerifier.port1);
  relay(toVerifier.port1, toProver.port1);

  channelStatus.textContent = "exchanging…";
  log(proverLog, p.proverStage());
  log(verifierLog, "blind slot allocated — bytes unknown");
  log(proverLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(verifierLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(proverLog, "executing on the zk-vm");
  log(verifierLog, "verifying every instruction…");
  workers.prover.postMessage(reqs.prover, [toProver.port2]);
  workers.verifier.postMessage(reqs.verifier, [toVerifier.port2]);
});

selectProgram("square");
