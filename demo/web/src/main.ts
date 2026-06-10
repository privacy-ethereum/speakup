import type { WorkerRequest, WorkerResponse } from "./worker";
import "./style.css";

const worker = new Worker(new URL("./worker.ts", import.meta.url), {
  type: "module",
});

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const runBtn = $<HTMLButtonElement>("run");
const xInput = $<HTMLInputElement>("x-input");
const proverLog = $("prover-log");
const verifierLog = $("verifier-log");
const channel = $("channel");
const channelStatus = $("channel-status");
const resultBox = $("result-box");
const resultEl = $("result");

const log = (el: HTMLElement, text: string, cls = "") => {
  const line = document.createElement("div");
  line.className = `log-line ${cls}`;
  line.textContent = text;
  el.appendChild(line);
  el.scrollTop = el.scrollHeight;
};

let ready = false;
runBtn.disabled = true;
channelStatus.textContent = "loading wasm…";

worker.onmessage = (ev: MessageEvent<WorkerResponse>) => {
  const msg = ev.data;
  switch (msg.type) {
    case "ready":
      ready = true;
      runBtn.disabled = false;
      channelStatus.textContent = "idle";
      log(proverLog, "zk-vm loaded");
      log(verifierLog, "zk-vm loaded");
      break;
    case "done": {
      channel.classList.remove("active");
      channelStatus.textContent = `proof complete in ${msg.ms.toFixed(0)} ms`;
      resultEl.textContent = String(msg.result);
      resultBox.hidden = false;
      log(proverLog, `revealed result: ${msg.result}`, "ok");
      log(verifierLog, `proof checked ✓ — result: ${msg.result}`, "ok");
      log(verifierLog, "the input x was never disclosed", "muted");
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
  const x = Number(xInput.value);
  if (!Number.isInteger(x)) {
    log(proverLog, "x must be an integer", "err");
    return;
  }
  runBtn.disabled = true;
  resultBox.hidden = true;
  channel.classList.add("active");
  channelStatus.textContent = "running protocol…";
  log(proverLog, `staging private input x = ${x}`);
  log(verifierLog, "blind slot allocated — bytes unknown");
  log(proverLog, "calling compute(x) on the zk-vm");
  log(verifierLog, "verifying every instruction…");
  const req: WorkerRequest = { type: "run", program: "square", x };
  worker.postMessage(req);
});
