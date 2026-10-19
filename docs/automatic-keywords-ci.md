# Optional model validation workflow

This workflow is proposed separately from local tagging. It runs only after an explicit **workflow_dispatch**; it adds no automatic pull-request, push or scheduled model downloads. Enabling its upstream runner cost remains a maintainer decision.

Each dispatch checks Windows, Linux and macOS, pins Rust 1.99.1 and both GitHub actions by commit, and uses a read-only token. Cargo dependencies and the WASM standard library are fetched before offline validation. Formatting, locked/offline Clippy and workspace tests, parity, layers, asset attribution and WASM checks run with the actual four pinned models present.

Cold runs download approximately 466 MB of model weights per operating system. A per-OS cache is keyed by the registry source containing pinned digests; restored files are still size/hash verified before use. The two application-installer regressions explicitly download a fresh 94 MB TinyCLIP file and a 3.7 MB YOLOX file to verify the real download path independently of the cache. CPU benchmark timings remain a local developer tool rather than a CI gate.

The 90-minute bound accommodates cold workspace compilation and offline gates; a maintainer can cancel an individual OS job. A prior workflow with a 45-minute bound completed its Windows tests, but its benchmark-timing step was cancelled. Missing corpus, fonts, optional face/SAM models or system data remain explicitly reported skips. This does not claim exhaustive model accuracy or guarantee external download availability.

Installer tests keep the workspace feature graph so they reuse the engine test binary, including SAM and denoise, instead of compiling another variant. The bounded hybrid integration wait includes cold model loading and competition with foreground render tests on shared runners; accuracy and persistence assertions are unchanged.
