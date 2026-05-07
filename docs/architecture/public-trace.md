# Public Trace

Public trace mode is used for regions of the program where the execution path can be resolved from public data alone. The VM unrolls the trace as execution proceeds and emits a straight-line circuit that matches the actual path taken.

Each executed instruction compiles to a minimal specialized circuit. An `add` becomes an adder; a shift becomes a shifter. There is no general-purpose ALU, so each operation uses only the gates it actually needs.

Register reads and writes are plain wire connections: a value produced by one instruction is wired directly to the instruction that consumes it, with no memory access or addressing overhead.

Linear memory still requires [ZK-RAM](memory-checking) *when the address is private*. Each such load or store carries the cost of one RAM access.

There is no program counter, ROM lookup, or CPU state commitment. The VM knows the exact instruction to execute at each point, so it skips the machinery that the [private trace](private-trace.md) CPU emulator needs.

Per-instruction cost depends on the operation. Instructions with only public operands collapse away entirely: both parties compute the result themselves, contributing nothing to the circuit. Otherwise, linear operations (XOR, register moves, shifts by public amounts) reduce to wire rewiring and are effectively free. Most other i32 instructions cost around 32 sVOLE, one committed bit per output bit. Private-address loads and stores carry the cost of one RAM access.

For comparison, a single [private trace](private-trace.md) step costs around 700 sVOLE regardless of the underlying instruction, roughly an order of magnitude more than a typical public-trace instruction.
