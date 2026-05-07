# SpeakUp

This document provides a **design draft** for a zero-knowledge virtual machine for proving arbitrary WebAssembly programs, called SpeakUp. Built on VOLE-based cryptography, it offers a distinct set of trade-offs compared to existing zkVMs:

- **Fast prover**: SpeakUp is designed for proving on resource-constrained devices like mobile phones and browsers. It trades larger proofs for minimal computational requirements on the prover.
- **Private**: alternatives focus on succinct publicly-verifiable proofs, with privacy often being a secondary concern. SpeakUp is designed specifically for privacy preserving applications.
- **Post-quantum secure**: the underlying cryptographic assumptions resist known quantum attacks, providing long-term security guarantees.
- **Simple**: SpeakUp relies on cryptographic constructions which are (relatively) simple to understand and to implement in software. This makes it easier to audit and reduces the likelihood of bugs.

SpeakUp is built on WebAssembly, enabling developers to write provable programs in any language that compiles to Wasm. WebAssembly provides a well-defined embedding interface which makes SpeakUp work naturally alongside other application code.

```{toctree}
:maxdepth: 2

introduction
proof-system
architecture/index
profile-viewer
```
