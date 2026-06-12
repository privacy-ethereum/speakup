// Cross-device transport: one reliable WebRTC DataChannel between the
// prover's page and the verifier's page, brokered by PeerJS.
//
// Only the short signaling handshake touches the PeerJS cloud broker; the
// protocol bytes flow peer-to-peer (on the same network, directly over the
// LAN). The page-level relay in main.ts stays the single crossing point:
// in remote mode it pumps the local party's MessagePort into this link
// instead of into the other local worker.
//
// Wire format: every DataChannel message is one ArrayBuffer,
//   byte 0        — frame tag (protocol bytes | control message)
//   bytes 1..     — payload
// Control messages are JSON with typed arrays encoded explicitly (the run
// requests carry Uint8Array / Uint32Array / BigInt64Array fields, which
// plain JSON can't represent). PeerJS's binary serialization chunks and
// reassembles frames larger than the ~16 KB DataChannel-safe MTU, so
// protocol messages and multi-hundred-KB control frames (the custom-wasm
// request) need no extra care here.

import { Peer, type DataConnection } from "peerjs";
import type { PartyRequest } from "./party.worker";

/// What the verifier device needs to label the result of a run it didn't
/// configure: the claim path/label and the expected value are public claim
/// data — exactly what a verifier agrees to check.
export interface RemoteDisplay {
  path?: string;
  label?: string;
  expect: string | null;
}

export type ControlMsg =
  /// First message after connect, host → guest (the guest's version
  /// travels in the connection metadata): both sides check that the two
  /// pages run the same deploy, i.e. embed identical guest modules.
  | { kind: "hello"; version: string }
  | {
      kind: "start";
      program: string;
      /// The verifier-side run request — public data only (private fields
      /// are empty/zeroed by requests(), same as in local mode).
      request: PartyRequest;
      /// The prover's blind() string for the "as the verifier sees it" cell.
      blind: string;
      /// Human lines for the verifier log (public inputs of this run).
      summary: string[];
      display: RemoteDisplay | null;
    }
  | { kind: "done"; result: string; ms: number }
  | { kind: "error"; message: string }
  | { kind: "abort" };

const TAG_PROTOCOL = 0;
const TAG_CONTROL = 1;

// --- typed-array-safe JSON ---

const b64encode = (bytes: Uint8Array): string => {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(s);
};

const b64decode = (s: string): Uint8Array => {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
};

const bytesOf = (v: ArrayBufferView) =>
  new Uint8Array(v.buffer, v.byteOffset, v.byteLength);

const replacer = (_k: string, v: unknown) => {
  if (v instanceof Uint8Array) return { __ta: "u8", b64: b64encode(v) };
  if (v instanceof Uint32Array) return { __ta: "u32", b64: b64encode(bytesOf(v)) };
  if (v instanceof BigInt64Array) return { __ta: "i64", b64: b64encode(bytesOf(v)) };
  return v;
};

const reviver = (_k: string, v: unknown) => {
  if (
    typeof v === "object" &&
    v !== null &&
    "__ta" in v &&
    "b64" in v &&
    typeof (v as { b64: unknown }).b64 === "string"
  ) {
    // b64decode returns a fresh, exactly-sized buffer, so the wider views
    // can sit directly on it.
    const bytes = b64decode((v as { b64: string }).b64);
    switch ((v as { __ta: string }).__ta) {
      case "u8":
        return bytes;
      case "u32":
        return new Uint32Array(bytes.buffer);
      case "i64":
        return new BigInt64Array(bytes.buffer);
    }
  }
  return v;
};

const frame = (tag: number, payload: Uint8Array): ArrayBuffer => {
  const out = new Uint8Array(1 + payload.byteLength);
  out[0] = tag;
  out.set(payload, 1);
  return out.buffer;
};

/// PeerJS hands received binary back as an ArrayBuffer in current builds,
/// but normalize views too rather than depend on it.
const asBytes = (d: unknown): Uint8Array | null => {
  if (d instanceof ArrayBuffer) return new Uint8Array(d);
  if (ArrayBuffer.isView(d)) return bytesOf(d);
  return null;
};

// --- the link ---

export interface RemoteEvents {
  onControl(msg: ControlMsg): void;
  onProtocol(data: ArrayBuffer): void;
  /// The connection (or the peer behind it) went away, for any reason.
  onClose(reason: string): void;
}

export class RemoteLink {
  /// The guest's pkg version, from the connection metadata (host side;
  /// empty on the guest, whose check uses the host's `hello`).
  readonly peerVersion: string;
  private closed = false;

  constructor(
    private peer: Peer,
    private conn: DataConnection,
    events: RemoteEvents,
  ) {
    const meta = conn.metadata as { version?: string } | undefined;
    this.peerVersion = meta?.version ?? "";
    conn.on("data", (d) => {
      const bytes = asBytes(d);
      if (!bytes || bytes.byteLength === 0) return;
      const payload = bytes.slice(1);
      if (bytes[0] === TAG_PROTOCOL) {
        events.onProtocol(payload.buffer);
      } else if (bytes[0] === TAG_CONTROL) {
        events.onControl(
          JSON.parse(new TextDecoder().decode(payload), reviver) as ControlMsg,
        );
      }
    });
    conn.on("close", () => this.teardown(events, "the remote device disconnected"));
    conn.on("error", (e) => this.teardown(events, `connection error: ${e.message}`));
    // No peer-level error handler here: once the DataChannel is up it no
    // longer depends on the broker, so a signaling blip must not kill a run.
  }

  sendControl(msg: ControlMsg) {
    if (this.closed) return;
    this.conn.send(
      frame(TAG_CONTROL, new TextEncoder().encode(JSON.stringify(msg, replacer))),
    );
  }

  sendProtocol(data: ArrayBuffer) {
    if (this.closed) return;
    this.conn.send(frame(TAG_PROTOCOL, new Uint8Array(data)));
  }

  close() {
    this.closed = true;
    this.peer.destroy();
  }

  private teardown(events: RemoteEvents, reason: string) {
    if (this.closed) return;
    this.closed = true;
    this.peer.destroy();
    events.onClose(reason);
  }
}

export interface LinkCallbacks {
  events: RemoteEvents;
  onConnected(link: RemoteLink): void;
  onError(message: string): void;
}

/// Host side (the prover's device): register with the broker, hand the
/// join URL to the caller (rendered as a QR code), wait for one guest.
export const hostInvite = (
  opts: LinkCallbacks & {
    joinUrl(id: string): string;
    onWaiting(url: string): void;
  },
): { cancel(): void } => {
  const peer = new Peer();
  let link: RemoteLink | null = null;
  peer.on("open", (id) => opts.onWaiting(opts.joinUrl(id)));
  peer.on("connection", (conn) => {
    if (link) {
      conn.close(); // one verifier at a time
      return;
    }
    conn.on("open", () => {
      link = new RemoteLink(peer, conn, opts.events);
      opts.onConnected(link);
    });
  });
  peer.on("error", (e) => {
    if (!link) opts.onError(e.message);
  });
  return {
    cancel() {
      if (!link) peer.destroy();
    },
  };
};

/// Guest side (the verifier's device): connect to the host id from the
/// scanned URL.
export const joinInvite = (
  hostId: string,
  version: string,
  opts: LinkCallbacks,
): void => {
  const peer = new Peer();
  let link: RemoteLink | null = null;
  peer.on("open", () => {
    const conn = peer.connect(hostId, {
      reliable: true,
      metadata: { version },
    });
    conn.on("open", () => {
      link = new RemoteLink(peer, conn, opts.events);
      opts.onConnected(link);
    });
  });
  peer.on("error", (e) => {
    // 'peer-unavailable' = the host closed the invite (or the link is stale).
    if (!link) {
      peer.destroy();
      opts.onError(e.type === "peer-unavailable" ? "the prover's invite is no longer open" : e.message);
    }
  });
};
