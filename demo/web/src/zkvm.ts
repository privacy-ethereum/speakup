// The demo's JavaScript zkVM API — a thin, role-aware facade over the raw
// `speakup-wasm` bindings (which expose only primitives: writePrivate,
// writeBlind, writePublic, call, callLocal, read).
//
// This layer is what the orchestration script in the editor is written
// against. Its job is to hide the prover/verifier split: a script calls
// `vm.writePrivate(ptr, bytes)` identically on both sides, and this wrapper
// dispatches to the right primitive (the prover contributes the bytes; the
// verifier reserves a blind region of the same length). The wasm bindings stay
// guest- and demo-agnostic; the orchestration conveniences live here.

import type { Party } from "../public/pkg/speakup_wasm";

export type Role = "prover" | "verifier";

/// A call argument. `value` is ignored for `blind`.
export interface Param {
  kind: "public" | "private" | "blind";
  ty: "i32" | "i64";
  value?: number;
}

/// A public call argument (known to both parties).
export const Public = (value: number, ty: "i32" | "i64" = "i32"): Param => ({
  kind: "public",
  ty,
  value,
});

/// A private call argument: the prover contributes the value; the verifier
/// blinds it automatically (see `Vm.#mapParams`), so scripts use `Private`
/// identically on both sides — there is no separate `Blind` to write.
export const Private = (value: number, ty: "i32" | "i64" = "i32"): Param => ({
  kind: "private",
  ty,
  value,
});

/// One exported function of a module, as reported by the `module_exports`
/// binding. `supported` means the zk-vm can call it (i32/i64 scalars only).
export interface ExportInfo {
  name: string;
  params: ("i32" | "i64" | "f32" | "f64")[];
  results: ("i32" | "i64" | "f32" | "f64")[];
  supported: boolean;
}

/// Conveniences available to orchestration scripts.
export const helpers = {
  hex: (bytes: Uint8Array) =>
    [...bytes].map((b) => b.toString(16).padStart(2, "0")).join(""),
  utf8: (s: string) => new TextEncoder().encode(s),
  text: (bytes: Uint8Array) => new TextDecoder().decode(bytes),
};

/// The role-free zkVM the orchestration script drives. Wraps one raw `Party`
/// (prover or verifier) and the role it was built for; the role is held here,
/// never exposed to the script.
export class Vm {
  #party: Party;
  #role: Role;

  constructor(party: Party, role: Role) {
    this.#party = party;
    this.#role = role;
  }

  /// Role dispatch for call arguments: the verifier never holds a private
  /// value, so every `private` param becomes `blind` of the same type. The
  /// prover passes them through; public params are unchanged. This is the
  /// param-level twin of `writePrivate`, so scripts use `Private(x)`
  /// identically on both sides.
  #mapParams(params: Param[]): Param[] {
    if (this.#role === "prover") return params;
    return params.map((p) => (p.kind === "private" ? { kind: "blind", ty: p.ty } : p));
  }

  /// Calls a local (non-interactive) export — a pointer getter or
  /// `cabi_realloc`. Returns the result value, or null.
  callLocal(name: string, params: Param[]): number {
    return this.#party.callLocal(name, this.#mapParams(params));
  }

  /// Calls an interactive export (one protocol round). Both parties must issue
  /// the matching call. Resolves to the result value.
  call(name: string, params: Param[]): Promise<number> {
    return this.#party.call(name, this.#mapParams(params));
  }

  /// Stages a private input at `ptr`. The prover contributes the real `bytes`;
  /// the verifier only reserves a blind region of the same length. This is the
  /// one place that branches on role — so scripts never have to.
  writePrivate(ptr: number, bytes: Uint8Array): void {
    if (this.#role === "prover") this.#party.writePrivate(ptr, bytes);
    else this.#party.writeBlind(ptr, bytes.length);
  }

  /// Stages public bytes (known to both parties) at `ptr`.
  writePublic(ptr: number, bytes: Uint8Array): void {
    this.#party.writePublic(ptr, bytes);
  }

  /// Reads `len` revealed bytes at `ptr`.
  read(ptr: number, len: number): Uint8Array {
    return this.#party.read(ptr, len);
  }

  /// The module's exported functions as JSON.
  exports(): string {
    return this.#party.exports();
  }
}

/// The auto-generated typed API for a loaded module: one method per export,
/// each calling that export interactively. So a script can write
/// `await guest.hash(Public(ptr), Public(len))` instead of
/// `vm.call("hash", [...])`. The editor autocompletes these from the module's
/// exports. (`call` is the universal path — it works for plain allocators too;
/// reach for `vm.callLocal` only to skip the protocol round.)
export type Guest = Record<string, (...params: Param[]) => Promise<number>>;

export const makeGuest = (vm: Vm, exportNames: string[]): Guest => {
  const guest: Guest = {};
  for (const name of exportNames) {
    guest[name] = (...params: Param[]) => vm.call(name, params);
  }
  return guest;
};
