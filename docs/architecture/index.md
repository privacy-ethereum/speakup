# Architecture

SpeakUp proves correct execution of WebAssembly programs. While running the program, the VM switches between two modes based on whether the current region of control flow can be resolved from public data.

[**Public trace**](public-trace.md) is used for regions where the execution path can be resolved from public data alone. Both parties can tell which instructions execute and in what order. The VM emits a straight-line circuit that matches the actual path taken: each instruction becomes a minimal specialized circuit (an `add` is just an adder), and register-to-register data flow is plain wiring. Operations on public data contribute nothing to the proof, so developers can write one program without needing to painstakingly pass data across the VM interface; only operations that touch private data cost anything.

[**Private trace**](private-trace.md) is used for regions where the execution path depends on private data. For example, a branch that calls one function when a private condition holds and a different function when it does not: the verifier would learn which branch was taken from the sequence of instructions that run. The VM hides the path by running a general-purpose CPU emulator: a sequence of uniform steps, each fetching a micro-instruction from ROM and executing the full step circuit. Every step pays for the full CPU machinery, regardless of how simple the underlying WebAssembly instruction is. For shallow private regions, the VM can skip the emulator and prove the region as a disjunction instead: all paths are evaluated together, and a private selector commits to which one actually ran.

Mode switching happens on the fly. A straight-line region runs in public trace; as soon as control flow depends on a private value, the VM enters private trace until execution reaches the next publicly resolvable join point (or the program exits). WebAssembly's structured control flow makes this tractable: every branch has a known join point, so the VM always knows where public-trace execution can resume.

The two modes have very different cost profiles. A private-trace instruction carries significant fixed overhead per step for the underlying CPU machinery. Programs that keep most of their work in public trace are dramatically cheaper than programs that spend much of their time in private trace.

```{toctree}
:hidden:

public-trace
private-trace
```
