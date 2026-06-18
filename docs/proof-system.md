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
  - $2^{16}$
  - sVOLE
* - Communication
  - $128$
  - KB
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

Let $n$ be the number of committed variables, $t$ the number of polynomials, $d$ the maximum degree, and $z$ the maximum number of terms in any single polynomial. The $n$ committed inputs are accounted for separately; the costs below are for the proof itself.

The communication is constant in $t$ and $z$ while computation grows linearly in the total number of polynomial terms.

::::{container} side-by-side

:::{container}

```{list-table} Communication
:header-rows: 1

* -
  - Cost
  - Unit
* - VOPE masking (one-time)
  - $(2d - 3) \cdot \lambda$
  - sVOLE
* - Coefficients ($\mathcal{P} \to \mathcal{V}$, one-time)
  - $d \cdot \lambda$
  - bits
* - Challenge ($\mathcal{V} \to \mathcal{P}$, one-time)
  - $\lambda$
  - bits
```

:::

:::{container}

```{list-table} Computation
:header-rows: 1

* -
  - Cost
  - Unit
* - Prover
  - $O(t \cdot d^2 \cdot z + d \cdot n)$
  - $\mathbb{F}_{2^\lambda}$ ops
* - Verifier
  - $O(t \cdot d \cdot z)$
  - $\mathbb{F}_{2^\lambda}$ ops
```

:::

::::

(memory-checking)=
## Memory Checking

SpeakUp requires random access memory to model the WebAssembly linear memory, registers, call stack, and global variables. For this it uses *offline memory checking*: the read/write grand product with timestamps introduced by Blum et al. (1994) and developed for verifiable computation by [Spice (Setty et al., 2018)](https://eprint.iacr.org/2018/907), and brought to VOLE-ZK by [Yang and Heath (2024)](https://eprint.iacr.org/2023/1115). SpeakUp instantiates this construction over binary extension fields and integrates it with the QuickSilver protocol described above.

The construction lets the prover read and write a memory of $n$ words over $T$ accesses, proving to the verifier that all accesses are consistent, without revealing the memory contents. Memory tuples (address, value, time) are [packed](field-packing) into $\mathbb{F}_{2^\kappa}$ elements for the permutation proof.

The construction reduces to two primitives:

1. **Permutation proof:** the list of tuples read from memory is a permutation of the list written, so every read is matched by a corresponding write.
2. **Timestamp comparison:** every read accesses a value written in the past, not the future.

Together they force every read to return the most recent prior write to its address: the strictly increasing clock makes per-address write times unique, so the read and write time-multisets match only when each read takes its preceding write.

Read-only memory is not a separate construction: a ROM is simply an instance whose every access is a read.

### Protocol

The memory maintains *reads* and *writes* vectors of (address, value, time) tuples and a public **clock** counter starting at 1. The clock is incremented outside the circuit, so all write times are public constants; only the times claimed for reads are prover inputs.

:::{admonition} Protocol: Memory
:class: protocol

**Setup.** For each address $i$ with initial value $\mathbf{x}[i]$, append $(i, \mathbf{x}[i], 0)$ to *writes*.

**Access.** On each access to address $\text{addr}$, with the clock at public value $c$:

1. **Inputs:** the prover inputs the old value $\text{old}$ and the time $t$ when $\text{addr}$ was last written.
2. **Timing check:** prove $t < c$ via the [comparator](timing-comparator).
3. **Write value:** determine the new value $\text{new}$ to write. For a read, $\text{new} = \text{old}$; for a write, $\text{new}$ is the value being stored.
4. **Append:** append $(\text{addr}, \text{old}, t)$ to *reads* and $(\text{addr}, \text{new}, c)$ to *writes*. The write time $c$ is a public constant.
5. **Clock:** increment the clock (public bookkeeping, free).

**Teardown.** For each address $i$, the prover inputs the final value and time. These are appended to *reads*. Then: prove *reads* $\sim$ *writes* (permutation proof).
:::

Every access both reads and writes: a write replaces the old value, and a read writes it back unchanged. The direction is known publicly (each step has fixed read/write slots), so step 3 is a free linear operation.

The teardown time inputs carry no timing check, and none is needed: the permutation proof forces them to equal the final write times.

(timing-comparator)=
#### Comparator

The timing check asserts $t < c$ for the public clock value $c$ and a committed timestamp $t$, represented in $b = \lceil \log(T + 1) \rceil$ bits, just enough for every clock value. Let $B = c - 1$, a public $b$-bit constant ($B \geq 0$, since $c \geq 1$ at every access). Equivalently, the subtraction $B - t$ over $b$ bits produces no final borrow. The borrow chain is

$$
\text{bor}_0 = t_0 \cdot \overline{B_0}, \qquad
\text{bor}_{j} = \begin{cases}
t_j \lor \text{bor}_{j-1} & B_j = 0 \\
t_j \land \text{bor}_{j-1} & B_j = 1
\end{cases}
$$

and the comparator asserts $\text{bor}_{b-1} = 0$. Because a committed timestamp *is* its $b$ bits, $t \in [0, 2^b)$ holds structurally; for operands in this range the borrow-out of $B - t$ is 1 iff $t > B$, so the check accepts exactly when $t \leq B$, i.e. $t < c$. Evaluating over exactly the operand width leaves no modular wrap-around to exploit.

### Accelerating Permutation Products

The permutation proofs require fan-in-$M$ products over $\mathbb{F}_{2^\kappa}$ of the form:

$$
p(r) = \prod_{i=1}^{M} (e_i + r)
$$

where $e_i$ are committed values and $r$ is a public challenge. The entries $e_i$ are memory tuples packed into $\mathbb{F}_{2^\kappa}$: when the total bit-width of a tuple exceeds $\kappa$, the tuple is compressed via a random linear combination with a verifier-supplied challenge vector (a free linear operation). This allows using $\kappa < \lambda$ for the permutation products, reducing cost. The soundness error per product is $(d + \lceil M/(d-1) \rceil) / 2^\kappa$.

In the gate-by-gate approach, each intermediate product must be committed, costing $\kappa$ sVOLEs per intermediate. As noted by Yang and Heath, QuickSilver's [polynomial proof protocol](polynomial-proofs) can accelerate these products. The products are split into chunks of $d - 1$ entries, where each chunk is a degree-$d$ polynomial verified using the protocol above. All chunks are batch-verified with a single VOPE correlation. The amortized cost per entry is $\kappa / (d - 1)$ sVOLEs, compared to $\kappa$ sVOLEs in the gate-by-gate approach.

### Cost

Let $W$ be the word size. Each access commits its inputs and runs the comparator, whose borrow chain adds $b - 1$ multiplications.

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
* - Comparator
  - $b - 1$
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
* - Permutation products
  - $2(n + T) \cdot \kappa / (d - 1)$
  - sVOLE
```

:::

::::

For a ROM with public contents, the teardown inputs drop to $n \cdot b$ and the permutation products to $(n + 2T) \cdot \kappa / (d - 1)$.

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
* - ROM lookup
  - $91$
* - RAM access
  - $96$
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

- Checking the Correctness of Memories — Blum, Evans, Gemmell, Kannan, Naor (1994).
- [Proving the Correct Execution of Concurrent Services in Zero-Knowledge](https://eprint.iacr.org/2018/907) — Setty, Angel, Gupta, Lee (2018).
- [Two Shuffles Make a RAM](https://eprint.iacr.org/2023/1115) — Yang, Heath (2024).

**VOLE Extension**

- [Ferret: Fast Extension for Correlated OT with Small Communication](https://eprint.iacr.org/2020/924) — Yang, Weng, Lan, Zhang, Wang (2020).
