# Private Trace

Private trace mode is used when a branch condition depends on a private value. The zkVM can no longer resolve the execution path at runtime, so it falls back to a general-purpose CPU emulator.

Execution proceeds as a sequence of **steps**, each with a fixed circuit structure. A micro-instruction ROM controls what each step does. Since the execution trace is private, the verifier does not know which instruction is fetched at each step — the PC and all micro-instruction fields are committed (private) values. This generality comes at a cost: every step pays for the full CPU machinery regardless of how simple the underlying instruction is.

## Overview

```{image} /_static/diagrams/arch-cpu.svg
:align: center
```

## CPU State

Each step commits to a small CPU state:

| Field | Bits | Purpose |
|-------|------|---------|
| PC | 16 | Program counter (up to 64K micro-instructions) |
| FP | 18 | Frame pointer (256-aligned, up to 1024 frames) |
| SP | 10 | Call stack pointer (up to 1024 call depth) |
| acc | 32 | Accumulator for multi-step instructions |
| carry | 1 | Carry flag for cross-step propagation |

Multi-step instructions use the accumulator to pass intermediate results between steps. The carry flag enables 64-bit operations and comparison results to propagate across steps.

## Micro-Instruction Format

Each micro-instruction is stored in the program ROM.

| Field | Bits | Description |
|-------|------|-------------|
| slot_a | 16 | Slot A index |
| slot_b | 16 | Slot B index |
| alu_ctrl | 15 | ALU control word |
| slot_a_mode | 3 | Slot A addressing mode |
| slot_b_mode | 3 | Slot B addressing mode |
| write_enable | 1 | Enable write-back on Slot B |
| write_select | 1 | Write ALU result / write `{31'b0, carry}` |
| acc_update | 2 | acc ← ALU result / Slot A value / unchanged |
| pc_mode | 2 | PC+1 / branch / conditional / indirect |
| sp_mode | 2 | SP hold / SP+1 / SP-1 |
| fp_update | 1 | FP hold / FP update |

### PC Modes

| Mode | Behavior |
|------|----------|
| Sequential | PC ← PC + 1 |
| Branch | PC ← slot_a |
| Conditional | PC ← carry ? slot_a : PC + 1 |
| Indirect | PC ← acc |

Branch targets use the `slot_a` field (16 bits = PC width).

## Memory

SpeakUp uses a single RAM instance with a unified address space. Each region is identified by a 3-bit mode prefix in the address key.

| Region | Mode | Contents | Capacity |
|--------|------|----------|----------|
| Registers | `REG` | Function-local registers | 256 regs × 1024 frames |
| Call stack | `STACK` | Return addresses and saved frame pointers | 1024 entries |
| Linear memory | `MEM` | WebAssembly linear memory | 64M cells (256 MB) |
| Constants | `CONST` | Compile-time constant values | 64K constants |
| Globals | `GLOBAL` | WebAssembly global variables | 64K globals |
| Tables | `TABLE` | Function tables for indirect calls | 64K entries |

All addresses are 29 bits: `mode[2:0] || base[9:0] || index[15:0]`. The base is selected from {FP[17:8], SP, acc[25:16], 0} and the index from {slot index, acc[15:0]}, depending on the mode.

Each step performs 2 RAM accesses:

- **Slot A** — read-only. Provides operand `a` to the ALU.
- **Slot B** — read-write. Provides operand `b` to the ALU. Conditionally writes back the ALU result.

Both slots support all addressing modes. This 2-operand architecture naturally fits WebAssembly's stack machine, where most binary operations consume both operands and a register allocator arranges for the result to overwrite one source.

## Step Circuit

### Step phases

Every step proceeds through five phases:

1. **ROM lookup** — fetch the micro-instruction for the current PC.
2. **Read** — compute the two slot addresses and read operand values from RAM.
3. **Compute** — the ALU produces a result and carry from the two operands.
4. **Write** — slot B conditionally writes back (ALU result, carry value, or pass-through of slot A).
5. **State update** — advance PC (sequential / branch / conditional / indirect), update SP / FP / acc / carry.

### ALU capabilities

The ALU is a set of submodules selected per micro-instruction. Configuration is part of the proof parameters: programs can trade per-step overhead for lower step counts on heavy operations.

Three baseline submodules handle most instructions in one step:

- **Carry chain** — add, sub, bitwise logic, comparison (via carry out). Chains across steps for 64-bit arithmetic.
- **Barrel shift** — all five WASM shift types in a single step.
- **Sub-word extract/insert** in the write-back path — byte/halfword loads and stores use the address's low two bits as a lane offset.

A multiplication submodule is optional and tunable: a compact bit-serial design spreads one `i32.mul` over many steps; a wider multiplier completes it in a few, with a larger base step cost.

## Cost

Each step commits a set of values across the step circuit, one ROM lookup, and two RAM accesses. Every committed bit costs 1 sVOLE. The step circuit is arithmetized as [polynomial constraints](polynomial-proofs) with intermediates folded into higher-degree equations, eliminating their commitments.

::::{container} side-by-side

:::{container}

```{list-table} Parameters
:header-rows: 1

* - Parameter
  - Value
  - Description
* - $T$
  - $2^{20}$
  - Steps
* - $n$
  - $2^{20}$
  - RAM entries (4MB)
* - $n_r$
  - $2^{16}$
  - ROM entries
```

:::

:::{container}

```{list-table} Cost per Step
:header-rows: 1

* - Component
  - sVOLE
* - ROM lookup
  - $112$
* - RAM access × 2
  - $381$
* - Step circuit
  - $\approx200$
* - **Total**
  - $\approx700$
```

:::

::::

## Instruction Step Counts

Most WebAssembly instructions compile to a single step: arithmetic, bitwise ops, shifts, comparisons, memory loads and stores (including sub-word variants), and control-flow branches all fit in one. 64-bit operations take two steps — one per word, with the carry chaining across. Multiplication and division are the expensive instructions, costing many steps each; their exact count depends on the ALU's multiplier submodule. A few instructions (`br_table`, `call`, `return`, `call_indirect`) scale with their operand count.

See the [Cost Explorer](../profile-viewer.md) for instruction distributions from real WebAssembly programs.
