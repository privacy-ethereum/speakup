import type { WorkerRequest, WorkerResponse } from "./worker";
import "./style.css";

const worker = new Worker(new URL("./worker.ts", import.meta.url), {
  type: "module",
});

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
  /// Returns the request, or an error message for invalid input.
  request(): WorkerRequest | string;
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
    request() {
      const x = Number(xInput.value);
      if (!Number.isInteger(x)) return "x must be an integer";
      return { type: "run", program: "square", x };
    },
    blind: () => "░░░░░░░░",
    proverStage: () => `staging private input x = ${xInput.value}`,
    render(result) {
      return {
        text: result,
        cls: "",
        log: `result: ${result}`,
      };
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
    request() {
      const birthdate = dateInput.value;
      if (!/^\d{4}-\d{2}-\d{2}$/.test(birthdate)) return "pick a birth date";
      return { type: "run", program: "age", birthdate, today: todayPacked() };
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
    request() {
      const bytes = new TextEncoder().encode(textInput.value);
      if (bytes.length === 0) return "message must not be empty";
      if (bytes.length > 4096) return "message too long (max 4096 bytes)";
      return { type: "run", program: "sha256", message: bytes };
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

let ready = false;
runBtn.disabled = true;
channelStatus.textContent = "loading wasm…";

worker.onmessage = (ev: MessageEvent<WorkerResponse>) => {
  const msg = ev.data;
  const p = PROGRAMS[current];
  switch (msg.type) {
    case "ready":
      ready = true;
      runBtn.disabled = false;
      channelStatus.textContent = "idle";
      break;
    case "done": {
      channel.classList.remove("active");
      channelStatus.textContent = `proof complete in ${msg.ms.toFixed(0)} ms`;
      const r = p.render(msg.result);
      resultEl.textContent = r.text;
      resultEl.className = `result ${r.cls}`;
      resultBox.hidden = false;
      log(proverLog, `revealed — ${r.log}`, "ok");
      log(verifierLog, `proof checked ✓ — ${r.log}`, "ok");
      log(verifierLog, `${p.secretName} was never disclosed`, "muted");
      runBtn.disabled = false;
      break;
    }
    case "error":
      channel.classList.remove("active");
      channelStatus.textContent = "failed";
      log(verifierLog, msg.message, "err");
      runBtn.disabled = false;
      break;
  }
};

runBtn.addEventListener("click", () => {
  if (!ready) return;
  const p = PROGRAMS[current];
  const req = p.request();
  if (typeof req === "string") {
    log(proverLog, req, "err");
    return;
  }
  runBtn.disabled = true;
  resultBox.hidden = true;
  clearLogs();
  blindCell.textContent = p.blind();
  channel.classList.add("active");
  channelStatus.textContent = "running protocol…";
  log(proverLog, p.proverStage());
  log(verifierLog, "blind slot allocated — bytes unknown");
  log(proverLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(verifierLog, "OT preprocessing (CO15 → KOS → Ferret)…");
  log(proverLog, "executing on the zk-vm");
  log(verifierLog, "verifying every instruction…");
  worker.postMessage(req);
});

selectProgram("square");
