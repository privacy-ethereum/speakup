// The protocol engine. Spawns one worker per party (isolated wasm memories),
// owns the live run state (traffic counters, the relay queue, the timer), and
// runs a proof either locally (a MessageChannel relay between the two workers)
// or remotely (one worker here, the other on a peer reached over a WebRTC link).
// It reports lifecycle out through `RunCallbacks` and never imports a view — the
// page wires those callbacks to the button, stats, and remote glue.
//
// This is the one piece with real lifecycle, so it's a class (like `Vm` and
// `RemoteLink`); everything else in the demo is a factory.

import type { PartyRequest, PartyResponse, Role } from "./party.worker";
import type { ExportInfo } from "./zkvm";
import type { InspectHost } from "./guest-module";
import { sha256Hex } from "./dom";
import type { RemoteLink } from "./remote";

/// Remote-verifier mode: this device is the prover (host), the verifier (guest),
/// or neither (local). Owned by the remote session; the controller reads it
/// (via `RunCallbacks.remoteMode`) to decide whether to relay locally or over a
/// link, and to mirror control messages to the peer.
export type RemoteMode =
  | { kind: "local" }
  | { kind: "host"; link: RemoteLink } // this device is the prover
  | { kind: "guest"; link: RemoteLink }; // this device is the verifier

type Dir = "prover→verifier" | "verifier→prover";

/// Where a relayed message lands: the other party's MessagePort in local mode,
/// the WebRTC link in remote mode. MessagePort satisfies this shape.
interface Sink {
  postMessage(data: ArrayBuffer, transfer: Transferable[]): void;
}

interface QueuedMsg {
  data: ArrayBuffer;
  to: Sink;
  dir: Dir;
}

interface Traffic {
  bytes: number;
}

interface RunState {
  pv: Traffic; // prover → verifier
  vp: Traffic; // verifier → prover
  start: number;
  results: Partial<Record<Role, string>>;
  ticker: number;
  queue: QueuedMsg[];
  pumping: boolean;
  /// Remote mode: where incoming protocol bytes are delivered (the local
  /// party's page-side port end) and their direction label.
  remoteIn?: { to: Sink; dir: Dir };
}

export interface RunCallbacks {
  /// Both workers have reported ready (call count tracked; 2 = both up).
  onReady(readyCount: number): void;
  onTraffic(pvBytes: number, vpBytes: number): void;
  /// A run completed: `ok` means the parties agreed. `result` and `elapsedMs`
  /// are meaningful only when `ok`.
  onFinished(ok: boolean, result: string, elapsedMs: number): void;
  /// A run failed (a worker rejected the proof).
  onFailed(message: string): void;
  /// A worker errored with no active run (e.g. wasm failed to load).
  onIdleError(message: string): void;
  /// A run was aborted; fresh workers are now loading.
  onAborted(): void;
  onExports(exports: ExportInfo[]): void;
  onExportsError(message: string): void;
  /// The current remote mode (owned by the remote session).
  remoteMode(): RemoteMode;
}

export class RunController {
  // One worker per party: two isolated wasm memories, so the secret physically
  // cannot be in the other's address space. Re-spawned on abort.
  private workers = {} as Record<Role, Worker>;
  private run: RunState | null = null;
  private nReady = 0;
  // Host side: the prover's run request, held until the verifier signals `ready`
  // (it doesn't start proving — or timing — until the verifier is set up).
  private pendingProverReq: PartyRequest | null = null;

  /// How the inspector reaches the prover worker.
  readonly inspectHost: InspectHost = {
    postInspect: (wasm) =>
      this.workers.prover.postMessage({ type: "inspect", wasm } satisfies PartyRequest),
  };

  constructor(private cb: RunCallbacks) {
    for (const role of ["prover", "verifier"] as const) this.spawnWorker(role);
  }

  // --- public API ---

  isRunning(): boolean {
    return this.run !== null;
  }

  readyCount(): number {
    return this.nReady;
  }

  /// Start a local two-party run: a fresh channel pair, relayed through the
  /// queue so the traffic counters keep working.
  startLocalRun(proverReq: PartyRequest, verifierReq: PartyRequest): void {
    const toProver = new MessageChannel();
    const toVerifier = new MessageChannel();
    const state = this.newRunState();
    this.run = state;
    this.markStart(state); // local: both workers are ready, so proving starts now

    const relay = (from: MessagePort, to: MessagePort, dir: Dir) => {
      from.onmessage = (ev) => {
        state.queue.push({ data: ev.data as ArrayBuffer, to, dir });
        this.pump(state);
      };
    };
    // The prover holds toProver.port2, the verifier toVerifier.port2 — so a
    // message arriving on toProver.port1 came FROM the prover, and so on.
    relay(toProver.port1, toVerifier.port1, "prover→verifier");
    relay(toVerifier.port1, toProver.port1, "verifier→prover");

    this.workers.prover.postMessage(proverReq, [toProver.port2]);
    this.workers.verifier.postMessage(verifierReq, [toVerifier.port2]);
  }

  /// Host side of a remote run: ship the verifier request (public data only) to
  /// the other device. The local prover is held until the verifier replies
  /// `ready` (see `attachProver`), so proving — and timing — starts only once
  /// the verifier has compiled its guest and set up. For a custom AssemblyScript
  /// guest, send the source (not the binary) for the verifier to compile and
  /// hash-check itself.
  async startRemoteRun(
    proverReq: PartyRequest,
    verifierReq: PartyRequest,
    tab: string,
    ascSource: string | null,
    link: RemoteLink,
  ): Promise<void> {
    if (ascSource !== null && verifierReq.type === "run") {
      const wasmHash = await sha256Hex(verifierReq.wasm);
      link.sendControl({
        kind: "propose",
        tab,
        request: { ...verifierReq, wasm: new Uint8Array(0) },
        source: { asc: ascSource, wasmHash },
      });
    } else {
      link.sendControl({ kind: "propose", tab, request: verifierReq });
    }
    this.pendingProverReq = proverReq; // attached when the verifier accepts (`ready`)
    const state = this.newRunState();
    this.run = state;
  }

  /// Host side: the verifier accepted and is set up. Begin proving now — the
  /// timer starts here, excluding the verifier's decision and compile.
  attachProver(link: RemoteLink): void {
    if (this.run && this.pendingProverReq) {
      this.markStart(this.run);
      this.attachRemoteRun(this.run, "prover", this.pendingProverReq, link);
      this.pendingProverReq = null;
    }
  }

  /// Guest side: signal `ready`, then begin verifying. The proof — and timer —
  /// begins now, so neither side counts the verifier's decision, compile, or
  /// setup.
  startVerifierRun(request: PartyRequest, link: RemoteLink): void {
    link.sendControl({ kind: "ready" });
    const state = this.newRunState();
    this.run = state;
    this.markStart(state);
    this.attachRemoteRun(state, "verifier", request, link);
  }

  /// Incoming protocol bytes from the peer. The `ready` handshake (control,
  /// ordered before any protocol byte) attaches the run before bytes flow, so
  /// there's nothing to buffer: deliver, or drop stray bytes from a run that
  /// already ended here.
  onRemoteProtocol(data: ArrayBuffer): void {
    if (!this.run?.remoteIn) return;
    this.run.queue.push({ data, to: this.run.remoteIn.to, dir: this.run.remoteIn.dir });
    this.pump(this.run);
  }

  /// The peer reported its result; record it under the other party and finish if
  /// both sides are in.
  onPeerDone(result: string): void {
    if (!this.run) return;
    const other: Role = this.cb.remoteMode().kind === "guest" ? "prover" : "verifier";
    this.run.results[other] = result;
    if (
      this.run.results.prover !== undefined &&
      this.run.results.verifier !== undefined
    ) {
      this.finishRun();
    }
  }

  /// Fail the active run with a message (e.g. the peer reported an error).
  fail(message: string): void {
    this.failRun(message);
  }

  /// Clear a pending/active run without respawning workers (e.g. the verifier
  /// declined before any proving started).
  cancel(): void {
    this.endRun();
  }

  /// The protocol can't be interrupted mid-computation: kill both workers and
  /// spawn fresh ones (they hold no state between runs).
  abort(notifyPeer: boolean): void {
    if (!this.run) return;
    if (notifyPeer) {
      const rm = this.cb.remoteMode();
      if (rm.kind !== "local") rm.link.sendControl({ kind: "abort" });
    }
    this.endRun();
    for (const role of ["prover", "verifier"] as const) {
      this.workers[role].terminate();
      this.spawnWorker(role);
    }
    this.nReady = 0;
    this.cb.onAborted(); // a fresh pair of workers is loading
  }

  // --- internals ---

  private newRunState(): RunState {
    const state: RunState = {
      pv: { bytes: 0 },
      vp: { bytes: 0 },
      start: 0, // stamped by markStart when the proof actually begins
      results: {},
      queue: [],
      pumping: false,
      ticker: 0,
    };
    state.ticker = window.setInterval(() => {
      if (this.run === state) this.cb.onTraffic(state.pv.bytes, state.vp.bytes);
    }, 100);
    return state;
  }

  /// Stamp the proof's start time, once. In remote mode this fires only after
  /// the `ready` handshake, so the timer excludes connection setup and compile.
  private markStart(state: RunState): void {
    if (state.start === 0) state.start = performance.now();
  }

  private forward(state: RunState, item: QueuedMsg): void {
    const t = item.dir === "prover→verifier" ? state.pv : state.vp;
    t.bytes += item.data.byteLength;
    item.to.postMessage(item.data, [item.data]);
  }

  private pump(state: RunState): void {
    if (state.pumping) return;
    state.pumping = true;
    const step = () => {
      if (this.run !== state) return; // run ended
      const item = state.queue.shift();
      if (!item) {
        state.pumping = false;
        return;
      }
      this.forward(state, item);
      queueMicrotask(step);
    };
    step();
  }

  /// Wires one remote run: the local party's port pumps into the link, and
  /// `remoteIn` tells `onRemoteProtocol` where to deliver incoming bytes. Both
  /// directions go through the relay queue, so the traffic counters keep working.
  private attachRemoteRun(
    state: RunState,
    role: Role,
    request: PartyRequest,
    link: RemoteLink,
  ): void {
    const chan = new MessageChannel();
    const outDir: Dir = role === "prover" ? "prover→verifier" : "verifier→prover";
    const inDir: Dir = role === "prover" ? "verifier→prover" : "prover→verifier";
    const sink: Sink = { postMessage: (data) => link.sendProtocol(data) };
    chan.port1.onmessage = (ev) => {
      state.queue.push({ data: ev.data as ArrayBuffer, to: sink, dir: outDir });
      this.pump(state);
    };
    state.remoteIn = { to: chan.port1, dir: inDir };
    this.workers[role].postMessage(request, [chan.port2]);
  }

  private endRun(): void {
    if (!this.run) return;
    clearInterval(this.run.ticker);
    this.run = null;
    this.pendingProverReq = null;
  }

  private finishRun(): void {
    if (!this.run) return;
    const { prover, verifier } = this.run.results;
    const elapsed = performance.now() - this.run.start;
    this.cb.onTraffic(this.run.pv.bytes, this.run.vp.bytes);
    const ok = prover !== undefined && verifier !== undefined && prover === verifier;
    this.endRun();
    if (ok) this.cb.onFinished(true, prover ?? "", elapsed);
    else this.cb.onFinished(false, "", elapsed);
  }

  private failRun(message: string): void {
    if (!this.run) return;
    this.endRun();
    this.cb.onFailed(message);
  }

  private spawnWorker(role: Role): void {
    const worker = new Worker(new URL("./party.worker.ts", import.meta.url), {
      type: "module",
    });
    worker.onmessage = (ev: MessageEvent<PartyResponse>) => {
      const msg = ev.data;
      switch (msg.type) {
        case "ready":
          this.nReady += 1;
          this.cb.onReady(this.nReady);
          break;
        case "done": {
          if (!this.run) return;
          this.run.results[role] = msg.result;
          const rm = this.cb.remoteMode();
          if (rm.kind !== "local") {
            rm.link.sendControl({ kind: "done", result: msg.result, ms: msg.ms });
          }
          if (
            this.run.results.prover !== undefined &&
            this.run.results.verifier !== undefined
          ) {
            this.finishRun();
          }
          break;
        }
        case "error": {
          if (!this.run) {
            this.cb.onIdleError(msg.message);
            break;
          }
          const rm = this.cb.remoteMode();
          if (rm.kind !== "local") {
            rm.link.sendControl({ kind: "error", message: msg.message });
          }
          this.failRun(msg.message);
          break;
        }
        case "exports": // only the prover worker is asked to inspect
          this.cb.onExports(msg.exports);
          break;
        case "exports-error":
          this.cb.onExportsError(msg.message);
          break;
      }
    };
    this.workers[role] = worker;
  }
}
