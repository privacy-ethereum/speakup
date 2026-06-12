# Browser Demo

A live demonstration of SpeakUp running entirely in your browser:

**<https://privacy-ethereum.github.io/speakup/demo/>**

```{image} _static/demo.png
:alt: The demo — prover pane on the left, verifier pane on the right
:target: https://privacy-ethereum.github.io/speakup/demo/
:width: 100%
```

The page runs a real two-party session, not a simulation. The prover and the
verifier each live in their own web worker with isolated WebAssembly
memories, and speak the actual [mpz](https://github.com/privacy-ethereum/mpz)
protocol over a `MessageChannel` — including real correlated randomness from
the OT stack (Chou-Orlandi base OT, KOS extension, Ferret expansion). The
page in between relays every message, so you can watch the protocol traffic
live: message counts, bytes on the wire, and a delay slider to slow the
exchange down.

The two panes make the designated-verifier setting tangible: the left pane
shows what the prover knows (the private input), the right pane what the
verifier learns — the public statement and the accept/reject result, nothing
else.

## Example programs

Eight guest programs — ordinary Rust compiled to WebAssembly, with their full
source viewable inside the demo — cover a range of statements:

| Guest | Statement |
| --- | --- |
| `square` | "(x+1)² of my private number is y" — the hello-world. |
| `age` | "I am over 18", without revealing the birth date. |
| `sha256` | "The SHA-256 digest of my private message is d", up to 64 KB. |
| `regex` | "My private string matches this public pattern", via an oblivious DFA. |
| `luhn` | "My private card number passes the Luhn checksum." |
| `csv` | "The average of one column of my private CSV reaches a public threshold" — the document is parsed inside the VM, branch-free. |
| `json` | A claim about one field of a private JSON document: assert its value or disclose it, revealing no other field. |
| `transcript` | A claim about a captured HTTPS exchange — e.g. "the API assigned my POST `id` = 101", with the request body hidden. |

A "custom wasm" tab additionally accepts any compiled guest module and runs
your own exported function over private and public scalar inputs.

## Practical notes

- Everything is client-side: a static page with no backend. Inputs never
  leave the browser — the "prover" and "verifier" workers are both yours.
- The WebAssembly is multithreaded (rayon pools on nested workers), so
  proving uses all available cores: a `square` proof completes in well under
  a second; sha-256 over 16 KB in a few seconds.
- Multithreading needs `SharedArrayBuffer` and therefore cross-origin
  isolation. GitHub Pages cannot send the required COOP/COEP headers, so a
  small service-worker shim injects them — the first visit reloads the page
  once, automatically.
- The demo's source lives in
  [`demo/`](https://github.com/privacy-ethereum/speakup/tree/main/demo) of
  this repository.
