# Introduction

Most zkVMs in use today rely on non-interactive proof systems built from SNARKs or STARKs. These systems produce succinct, publicly verifiable proofs (properties essential for blockchain scaling), but impose significant computational cost on the prover. This makes them impractical for client-side devices like mobile phones and browsers, where compute and memory budgets are tight. For many practical applications, public verifiability and succinctness are unnecessary. Web proofs, identity attestations, and private credential verification all operate in the interactive designated-verifier setting, where a prover and verifier communicate directly. Yet the ecosystem has largely optimized for the on-chain case, leaving a wide gap for these off-chain settings.

SpeakUp fills this gap with an interactive proof system based on VOLE (Vector Oblivious Linear Evaluation). Without the heavy cryptographic machinery SNARKs and STARKs rely on, the prover scales linearly with program size and is light enough to run on mobile phones, in browser extensions, and on other resource-constrained devices.

The trade-off is larger proofs and a linear-time verifier. In an interactive setting these are a natural fit: communication streams between prover and verifier, and verification happens as messages arrive. The construction is also post-quantum secure.

## Applications

SpeakUp is a general-purpose tool for any application where two parties need to exchange a private proof. Whenever a prover and verifier are both online and can interact, SpeakUp can be used to prove arbitrary statements about private data without revealing it. Two particularly fitting applications are:

- **Web proofs**: proving properties about [data received over TLS](https://tlsnotary.org) without revealing the underlying content.
- **Identity and anonymous credentials**: a user proves properties about their credentials (age, membership, eligibility) to a verifier without disclosing the credentials themselves.

These are just specific instances of a broad pattern: any setting where one party needs to convince another of a computational statement involving private data, without a need for the proof to be publicly verifiable or stored on a blockchain.

## WebAssembly as Architecture

SpeakUp uses WebAssembly as its instruction set architecture. WebAssembly is a safe, portable, low-level code format with formally defined semantics, structured control flow, and a capability-based security model. It is also a first-class compilation target for LLVM, so developers can write provable programs in general-purpose languages like Rust or C, without touching circuit languages or DSLs.

Using WebAssembly as the input format brings several advantages:

- **Programmability**: developers work in familiar languages with existing libraries and tooling. They are not exposed to finite fields, constraint systems, or proving-system internals.
- **Portability**: a program compiled to WebAssembly runs in any environment, independent of the underlying proving system.
- **Tooling**: WebAssembly already has a mature ecosystem of compilers, optimizers, and analyzers. New tooling we build for proving can be reused by any other implementation that targets WebAssembly.
- **Formal semantics**: typing, validation, and execution are specified precisely enough to mechanize in proof assistants. This matters when reasoning about program behavior or compiler correctness.

On top of WebAssembly, SpeakUp implements the [Verifiable Compute](https://sinui0.github.io/vc-spec/) specification, a draft standard for two-party verifiable computation. Verifiable Compute defines a standard interface between application code and the underlying proving system. Developers write against this interface, and any conforming implementation can run their program. Application code stays portable across proving systems, with no lock-in.
