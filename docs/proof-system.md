# Proof System

SpeakUp's proof system is built from existing VOLE-based zero-knowledge protocols. It does not introduce new cryptographic constructions — instead, it combines and applies established techniques to prove correct execution of WebAssembly programs. This section describes these building blocks and how SpeakUp uses them. For the circuit designs that encode WebAssembly execution, see {doc}`architecture/index`.

## Notation

- $\mathcal{P}$ — prover. $\mathcal{V}$ — verifier.
- $\lambda$ — extension field degree for MACs and keys ($\mathbb{F}_{2^\lambda}$).
- $\kappa$ — extension field degree for permutation products ($\mathbb{F}_{2^\kappa}$, $\kappa \leq \lambda$).
- $[x]$ — a committed value. See [Commitment Scheme](commitment-scheme).
- **sVOLE** — one committed $\mathbb{F}_2$ value. All costs are stated in sVOLEs.

## Overview

The prover holds a witness $\mathbf{w}$ and wants to convince the verifier that a circuit $C$ evaluates correctly on $\mathbf{w}$, without revealing anything about it. SpeakUp represents computations as Boolean circuits over $\mathbb{F}_2$.

The proof system follows the VOLE-based ZK paradigm established by [Wolverine](https://eprint.iacr.org/2020/925), [Mac'n'Cheese](https://eprint.iacr.org/2020/1410), and [QuickSilver](https://eprint.iacr.org/2021/076). Its key properties are:

- **Linear operations are free.** Only multiplications (AND gates) consume resources.
- **Streaming.** Wire values can be discarded once no longer needed, keeping memory proportional to circuit width.
- **Preprocessing/online split.** VOLE correlations are generated ahead of time; the online phase consumes them as the circuit is evaluated.

## VOLE and Commitments

### VOLE Correlation

Vector Oblivious Linear Evaluation (VOLE) is a two-party primitive that produces correlated random values. A VOLE correlation consists of vectors $\mathbf{u}$, $\mathbf{v}$, $\mathbf{w}$ and a scalar $\Delta$ satisfying:

$$
\mathbf{u} = \mathbf{w} \cdot \Delta + \mathbf{v}
$$

The prover holds $(\mathbf{w}, \mathbf{u})$ while the verifier holds $(\mathbf{v}, \Delta)$. Crucially, $\mathcal{P}$ does not learn $\Delta$ and $\mathcal{V}$ does not learn $\mathbf{w}$.

VOLE correlations are generated in the preprocessing phase using any suitable VOLE extension protocol. The current state of the art is [Ferret](https://eprint.iacr.org/2020/924) (Yang et al., 2020), which uses the Learning Parity with Noise (LPN) assumption to expand a small seed VOLE into a large volume of correlations. The seed VOLE itself can be generated using [SoftSpokenOT](https://eprint.iacr.org/2022/192) (Roy, 2022), which operates in the minicrypt model and relies only on symmetric primitives (e.g., AES, SHA).

(commitment-scheme)=
### Commitment Scheme

VOLE correlations give rise to a homomorphic commitment scheme, first formalized in this context by [BDOZ11](https://eprint.iacr.org/2010/514) and [NNOB12](https://eprint.iacr.org/2011/091).

Each sVOLE correlation produces a **random** committed bit $\mu \in \mathbb{F}_2$ — the prover does not choose its value. To commit to a chosen value $x$, the prover sends the correction $\delta = x \oplus \mu$ to the verifier. Both parties then adjust the MAC and key to obtain a commitment to $x$. This **derandomization** costs one bit of communication per sVOLE. Throughout this document, the cost of an sVOLE includes this derandomization.

After derandomization, the prover holds $M[x] \in \mathbb{F}_{2^\lambda}$ and the verifier holds $K[x] \in \mathbb{F}_{2^\lambda}$, satisfying:

$$
M[x] = x \cdot \Delta + K[x]
$$

The committed value $x$ lives in $\mathbb{F}_2$, while the MAC, key, and global key all live in $\mathbb{F}_{2^\lambda}$ — this is what provides statistical security. We denote a committed value as $[x]$.

This scheme is **linearly homomorphic**: given committed values $[x]$ and $[y]$ and public constants $c_1, c_2, c$, both parties can locally compute $[c_1 \cdot x + c_2 \cdot y + c]$ without interaction. This is why linear operations (XOR, NOT) are free — they are linear operations on committed values.

Multiplications require interaction and are the primary cost driver.

(field-packing)=
### Field Packing

Multiple committed $\mathbb{F}_2$ values can be **packed** into a single committed $\mathbb{F}_{2^\lambda}$ value for free — this is a local operation requiring no interaction. Committing one $\mathbb{F}_{2^\lambda}$ value therefore costs $\lambda$ sVOLEs.

### Cost

Costs below are for Ferret (regular LPN), the current state-of-the-art sVOLE generation protocol.

::::{container} side-by-side

:::{container}

```{list-table} Per sVOLE (amortized @ $10^7$)
:header-rows: 1

* -
  - Cost
  - Unit
* - Communication ($\mathcal{V} \to \mathcal{P}$)
  - $0.44$
  - bits
* - Communication ($\mathcal{P} \to \mathcal{V}$)
  - $1$
  - bits
* - Computation
  - $\approx20$
  - ns
```

:::

:::{container}

```{list-table} One-Time Setup
:header-rows: 1

* -
  - Cost
  - Unit
* - Seed sVOLEs
  - $\approx 48K$
  - sVOLE
* - Communication
  - $\approx 1.1$
  - MB
```

:::

::::

The $\mathcal{V} \to \mathcal{P}$ cost above reflects only the generation of random sVOLE correlations. Derandomization adds $1$ bit $\mathcal{P} \to \mathcal{V}$ per sVOLE.

(multiplication-check)=
## Multiplication Check

SpeakUp uses the [QuickSilver](https://eprint.iacr.org/2021/076) protocol (Yang et al., 2021) to verify that committed multiplication gates are computed correctly. For each gate $z = x \cdot y$, the prover commits the output wire using one sVOLE. Both parties then derive a value from their respective MACs and keys — if the prover cheated ($z \neq x \cdot y$), the MAC algebra is inconsistent and the check fails.

All gate checks are batched: individual checks are combined with a random challenge into a single verification at the end. The soundness error is $(t + 3) / 2^\lambda$, and the batch check can be made non-interactive by deriving the challenge from a hash of the transcript.

### Cost

Let $t$ be the number of multiplications.

```{list-table} Cost
:header-rows: 1

* -
  - Cost
  - Unit
* - Multiplications
  - $t$
  - sVOLE
* - VOPE masking (one-time)
  - $\lambda$
  - sVOLE
* - Batch check ($\mathcal{P} \to \mathcal{V}$, one-time)
  - $2\lambda$
  - bits
* - Batch check ($\mathcal{V} \to \mathcal{P}$, one-time)
  - $\lambda$
  - bits
```

(polynomial-proofs)=
## Polynomial Proofs

The multiplication check above is a special case of a more general technique: each gate relation $z = x \cdot y$ is a degree-2 polynomial in the committed variables. QuickSilver generalizes this to degree-$d$ polynomial relations, where a set of $t$ polynomials over $n$ committed variables can be proved with communication of only $d$ field elements over $\mathbb{F}_{2^\lambda}$, independent of the number of multiplications in the polynomials.

As with the multiplication check, all polynomial checks are batched into a single verification. The soundness error is $(d + t) / 2^\lambda$.

### Cost

Let $n$ be the number of committed variables, $t$ the number of polynomials, and $d$ the maximum degree.

```{list-table} Cost
:header-rows: 1

* -
  - Cost
  - Unit
* - VOPE masking
  - $(2d - 3) \cdot \lambda$
  - sVOLE
* - Coefficients ($\mathcal{P} \to \mathcal{V}$)
  - $d \cdot \lambda$
  - bits
* - Challenge ($\mathcal{V} \to \mathcal{P}$)
  - $\lambda$
  - bits
```

(memory-checking)=
## Memory Checking

SpeakUp requires random access memory to model the WebAssembly linear memory, registers, call stack, and global variables. For this, SpeakUp applies the two-shuffle ZK-RAM construction of [Yang and Heath (2023)](https://eprint.iacr.org/2023/1099), which was originally presented over prime fields. SpeakUp adapts this construction to work over binary extension fields and integrates it with the QuickSilver protocol described above.

The ZK-RAM allows the prover to read and write to a memory of $n$ elements over the course of $T$ accesses, proving to the verifier that all accesses are consistent — without revealing the memory contents. Memory tuples (address, value, version/time) are [packed](field-packing) into $\mathbb{F}_{2^\kappa}$ elements for the permutation proofs.

The construction reduces to two core primitives:

1. **Permutation proofs** — proving that the list of values read from memory is a permutation of the list of values written, ensuring every write is matched by a corresponding read.
2. **Set membership** — proving that each read accesses a value written in the past, not the future.

### Adaptation to Binary Fields

The two-shuffle construction of Yang and Heath was originally designed for prime fields where $|F| \geq 2T$. Working over binary extension fields $\mathbb{F}_{2^\lambda}$ requires a straightforward adaptation: the permutation proofs carry over directly, but the timing checks require integer arithmetic — which has no direct analogue in $\mathbb{F}_{2^\lambda}$ since field addition is XOR, not integer addition. The required integer operations (increment and subtraction) are implemented via standard Boolean adder circuits operating on $b$-bit integers represented as authenticated bits. Both operations cost at most $(b - 2)$ multiplications.

The bit-width $b$ must satisfy $2^b \geq 2T$ to prevent wrap-around attacks on the timing check, so $b = \lceil \log(2T) \rceil$.

### Accelerating Permutation Products

The permutation proofs require fan-in-$M$ products over $\mathbb{F}_{2^\kappa}$ of the form:

$$
p(r) = \prod_{i=1}^{M} (e_i + r)
$$

where $e_i$ are committed values and $r$ is a public challenge. The entries $e_i$ are memory tuples packed into $\mathbb{F}_{2^\kappa}$: when the total bit-width of a tuple exceeds $\kappa$, the tuple is compressed via a random linear combination with a verifier-supplied challenge vector (a free linear operation). This allows using $\kappa < \lambda$ for the permutation products, reducing cost. The soundness error per product is $(d + \lceil M/(d-1) \rceil) / 2^\kappa$.

In the gate-by-gate approach, each intermediate product must be committed, costing $\kappa$ sVOLEs per intermediate. As noted by Yang and Heath, QuickSilver's [polynomial proof protocol](polynomial-proofs) can accelerate these products. The products are split into chunks of $d - 1$ entries, where each chunk is a degree-$d$ polynomial verified using the protocol above. All chunks are batch-verified with a single VOPE correlation. The amortized cost per entry is $\kappa / (d - 1)$ sVOLEs, compared to $\kappa$ sVOLEs in the gate-by-gate approach.

### Read-Only Memory (ROM)

ROM is the simpler building block. It stores $n$ key-value pairs and supports lookups that return the value associated with a key.

The ROM maintains two vectors of tuples: *reads* and *writes*. Each tuple contains a key, a value, and a **version** — an address-specific counter that increments on each lookup.

:::{admonition} Protocol: ROM
:class: protocol

**Setup.** For each address $i \in [n]$ with value $\mathbf{x}[i]$, append $(i, \mathbf{x}[i], 0)$ to *writes*. These are public wires — the prover does not control the initial versions.

**Lookup.** On each lookup of key $i$, the prover inputs the value $\mathbf{x}[i]$ and the current version $v$. The circuit computes $v + 1$ and appends:

- $(i, \mathbf{x}[i], v)$ to *reads*
- $(i, \mathbf{x}[i], v + 1)$ to *writes*

**Teardown.** For each address $i$, the prover inputs the final version $v_i$, and $(i, \mathbf{x}[i], v_i)$ is appended to *reads*. Finally, the circuit checks *reads* $\sim$ *writes* (permutation proof).
:::

**Soundness.** The permutation check forces the prover into building per-key version chains: setup writes version 0 (on public wires, not prover inputs), the first lookup reads 0 and writes 1, the second reads 1 and writes 2, and so on. The prover cannot forge initial values because version 0 is written by the circuit. The prover cannot skip versions because every write must have a matching read.

#### Cost

Let $n$ be the number of entries, $T$ the number of lookups, and $b = \lceil \log(T + 1) \rceil$ the version bit-width. The $n$ setup writes are publicly known, so their contribution to the writes product is computed locally by both parties.

::::{container} side-by-side

:::{container}

```{list-table} Per Lookup
:header-rows: 1

* -
  - Cost
  - Unit
* - Inputs: value + version
  - $W + b$
  - sVOLE
* - Version increment
  - $b - 2$
  - sVOLE
```

:::

:::{container}

```{list-table} Teardown (one-time)
:header-rows: 1

* -
  - Cost
  - Unit
* - Final version inputs
  - $n \cdot b$
  - sVOLE
* - Permutation products
  - $(n + 2T) \cdot \kappa / (d - 1)$
  - sVOLE
```

:::

::::

(set-membership)=
### Set Membership

Set membership is a specialization of ROM with no values — keys only. Given a public set $S = \{s_1, \ldots, s_m\}$, it proves that a private value $x$ belongs to $S$. The RAM uses a set with $S = \{1, \ldots, T\}$ to enforce timing constraints.

:::{admonition} Protocol: Set Membership
:class: protocol

**Setup.** For each key $s \in S$, append $(s, 0)$ to *writes*.

**Prove-member.** To prove $x \in S$, the prover inputs the current version $v$ for key $x$. The circuit computes $v + 1$ and appends:

- $(x, v)$ to *reads*
- $(x, v + 1)$ to *writes*

**Teardown.** For each key $s \in S$, the prover inputs the final version. Append to *reads*. Check *reads* $\sim$ *writes* (permutation proof).
:::

If $x \notin S$, no setup entry exists for $x$, the version chain has no root, and the permutation check fails.

#### Cost

Set membership is ROM with no values. Let $m = |S|$, $T$ the number of queries, and $b = \lceil \log(T + 1) \rceil$. The $m$ setup writes are publicly known, so their contribution to the writes product is computed locally.

::::{container} side-by-side

:::{container}

```{list-table} Per Query
:header-rows: 1

* -
  - Cost
  - Unit
* - Version input
  - $b$
  - sVOLE
* - Version increment
  - $b - 2$
  - sVOLE
```

:::

:::{container}

```{list-table} Teardown (one-time)
:header-rows: 1

* -
  - Cost
  - Unit
* - Final version inputs
  - $m \cdot b$
  - sVOLE
* - Permutation products
  - $(m + 2T) \cdot \kappa / (d - 1)$
  - sVOLE
```

:::

::::

### Read/Write Memory

The RAM maintains *reads* and *writes* vectors of (address, value, time) tuples, a **set** $\{1, \ldots, T\}$ for timing checks, and a public **clock** counter starting at 1.

:::{admonition} Protocol: Read/Write Memory
:class: protocol

**Setup.** For each address $i$ with initial value $\mathbf{x}[i]$, append $(i, \mathbf{x}[i], 0)$ to *writes*. Initialize the timing set with keys $\{1, \ldots, T\}$.

**Access.** On each access to address $\text{addr}$:

1. The prover inputs the old value $\text{old}$ and the time $t$ when $\text{addr}$ was last written.
2. **Timing check:** Compute $\text{diff} = \text{clock} - t$ and prove $\text{diff} \in \{1, \ldots, T\}$ via set membership.
3. **Write value:** Determine the new value $\text{new}$ to write. For a read, $\text{new} = \text{old}$. For a write, $\text{new}$ is the value being stored.
4. Append $(\text{addr}, \text{old}, t)$ to *reads* and $(\text{addr}, \text{new}, \text{clock})$ to *writes*.
5. Increment clock.

**Teardown.** For each address $i$, the prover inputs the final value and time. These are appended to *reads*. Then: prove *reads* $\sim$ *writes* and teardown the timing set.
:::

The access direction (read vs write) is known publicly — each step has fixed read/write slots — so step 3 is a linear operation and costs nothing.

**Soundness.** The RAM invariant: before each access to address $i$, *writes* contains exactly one tuple $(i, \text{val}, t)$ not yet matched in *reads*, recording the most recent write.

- **Permutation check** ensures every write is matched by a read and vice versa.
- **Set membership check** ensures $\text{clock} - t \in \{1, \ldots, T\}$, meaning the claimed write time $t$ is strictly in the past. This prevents the prover from reading values written in the future.
- Together, these imply the prover must read the most recent write: older writes were already consumed by previous reads and cannot be reused.

#### Cost

Let $W$ be the word size and $b = \lceil \log(2T) \rceil$. The tables below inline the timing [set membership](set-membership) check ($m = T$).

::::{container} side-by-side

:::{container}

```{list-table} Per Access
:header-rows: 1

* -
  - Cost
  - Unit
* - Inputs: old value + time
  - $W + b$
  - sVOLE
* - Timing subtraction
  - $b - 2$
  - sVOLE
* - Timing set: version input
  - $b$
  - sVOLE
* - Timing set: version increment
  - $b - 2$
  - sVOLE
```

:::

:::{container}

```{list-table} Teardown (one-time)
:header-rows: 1

* -
  - Cost
  - Unit
* - Final value + time inputs
  - $n \cdot (W + b)$
  - sVOLE
* - Timing set: final version inputs
  - $T \cdot b$
  - sVOLE
* - Permutation products (RAM + timing set)
  - $(2n + 5T) \cdot \kappa / (d - 1)$
  - sVOLE
```

:::

::::

## Concrete Cost

This section instantiates the parametric costs with concrete values to provide headline figures for modeling the zkVM.

::::{container} side-by-side

:::{container}

```{list-table} Parameters
:header-rows: 1

* - Parameter
  - Value
  - Description
* - $\lambda$
  - $128$
  - Extension field degree
* - $\kappa$
  - $64$
  - Permutation product field degree
* - $d$
  - $16$
  - Permutation check batching degree
* - $W$
  - $32$
  - Memory word size (bits)
* - $T$
  - $2^{23}$
  - Memory accesses
* - $n$
  - $2^{20}$
  - Memory entries (4MB)
* - $b$
  - $24$
  - Memory timing bit-width
```

:::

:::{container}

```{list-table} Cost (amortized)
:header-rows: 1

* - Operation
  - sVOLE
* - $\mathbb{F}_2$ input
  - $1$
* - $\mathbb{F}_2$ multiplication
  - $1$
* - Set membership (timing, $m = T$)
  - $83$
* - ROM lookup
  - $90$
* - RAM access
  - $177$
```

:::

::::

## Resources

The following papers provide the foundations for SpeakUp's proof system:

**Surveys**

- [SoK: Vector OLE-Based Zero-Knowledge Protocols](https://eprint.iacr.org/2023/857) — Baum, Dittmer, Scholl, Wang (2023).

**Proof Systems**

- [QuickSilver: Efficient and Affordable Zero-Knowledge Proofs for Circuits and Polynomials over Any Field](https://eprint.iacr.org/2021/076) — Yang, Sarkar, Weng, Wang (2021).
- [Wolverine: Fast, Scalable, and Communication-Efficient Zero-Knowledge Proofs for Boolean and Arithmetic Circuits](https://eprint.iacr.org/2020/925) — Weng, Yang, Katz, Wang (2021).
- [Mac'n'Cheese: Zero-Knowledge Proofs for Boolean and Arithmetic Circuits with Nested Disjunctions](https://eprint.iacr.org/2020/1410) — Baum, Malozemoff, Rosen, Scholl (2021).

**RAM and Memory Checking**

- [Two-Shuffles: From RAM to Secure Computation and Back](https://eprint.iacr.org/2023/1099) — Yang, Heath (2023).

**VOLE Extension**

- [Ferret: Fast Extension for Correlated OT with Small Communication](https://eprint.iacr.org/2020/924) — Yang, Weng, Lan, Wang (2020).
