# SpeakUp

Documentation and browser demo for the SpeakUp zkVM.

- [`docs/`](docs/) — the Sphinx documentation site, deployed to
  <https://privacy-ethereum.github.io/speakup/>.
- [`demo/`](demo/) — a live in-browser demo (prover and verifier as web
  workers over the real protocol), deployed alongside the docs at
  <https://privacy-ethereum.github.io/speakup/demo/>. See
  [`demo/README.md`](demo/README.md).

## Building the docs

```
uv run sphinx-build docs docs/_build/html
```

For live-reloading during development:

```
uv run sphinx-autobuild docs docs/_build/html
```

## Acknowledgements

Documentation styling adapted from the [WebAssembly Specification](https://github.com/WebAssembly/spec).
