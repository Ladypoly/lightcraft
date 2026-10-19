# Automatic tag platform validation

Validation date: 2026-10-19. Local evidence predates this review split.

The same four hash-pinned models and six public/CC0 photo fixtures are used across systems.
Model inference, source checksums, metadata search, manual keyword separation, review decisions,
catalog reopening, current-view scope, download installation and headless tagging UI are checked.
Both CLIP models are compared with Transformers references and both YOLOX graphs with ONNX Runtime
references. Product inference uses pure Rust CPU code; Python is development tooling only.

## Coverage

- Windows x86-64: 32 focused CPU/model/catalog/engine/download/UI checks passed; native app smoke test passed.
- Linux x86-64: the same 32 focused checks passed on Ubuntu 24.04 under WSL2, with GPU disabled and a headless UI.
- macOS Apple Silicon and Intel: tagging-crate cross-compilation checks only.
- Hosted macOS and Linux model/metadata jobs passed at `bc5cd205` in [the pre-split workflow](https://github.com/Ladypoly/lightcraft/actions/runs/39276418963); Windows passed tests and was cancelled during CPU timings. These are not full-workspace results on this branch.

Model-validation CI is proposed separately, with explicit dispatch, pinned Rust and cached verified weights; enabling upstream workflow cost is a maintainer decision.

Windows also passed 114 keyword UI regression tests (2 existing opt-in tests ignored) and 41
catalog keyword unit tests. The final AI metadata separation tests passed on both systems, covering
searchable predictions outside ordinary keyword lists, independent keyword edits, accepted-tag
provenance, source invalidation and reopening. The native All Metadata viewer was inspected with
readable model names and source-file checksums unchanged.

## Resource measurements

Both systems ran all four models on two logical CPUs, with `RAYON_NUM_THREADS=2` and GPU use disabled.
Linux additionally enforced a 4 GiB cgroup memory limit, zero swap and a 200% CPU quota.

| Model | Windows median/image | Windows peak working set | Linux median/image | Linux peak RSS |
|---|---:|---:|---:|---:|
| tinyclip-8m | 168 ms | 190 MiB | 167 ms | 187 MiB |
| tinyclip-39m | 513 ms | 643 MiB | 399 ms | 644 MiB |
| yolox-nano | 161 ms | 23 MiB | 133 ms | 30 MiB |
| yolox-s | 777 ms | 85 MiB | 543 ms | 98 MiB |

All six fixture tag sets were identical across Windows and Linux for every model. Windows and Linux
memory metrics are OS-specific; these figures describe isolated model processes, not the full app.
Under the Linux resource limit, all 8 model/reference tests and 11 engine tagging tests passed,
including real background indexing, search, reopening, cancellation/stale guards and source checksums.
Two opt-in network tests were ignored in that constrained engine run; the fresh YOLOX network download
was exercised separately on both systems. The 94 MB CLIP network test was not repeated; all four
cached model files were independently verified against product sizes and SHA-256 digests.

These are smoke measurements, not a hardware comparison or a broad accuracy benchmark. Both operating systems share an i9-13900K host; resource limits do not reproduce an older laptop.

The benchmark runs each model in a fresh process, with three passes over six already decoded photos
(18 image samples). It includes preprocessing and scoring, excludes image decoding/rendering,
catalog/UI overhead and downloads, and does not establish cold-disk load time. Tagging is CPU-only.

## Remaining checks

Native macOS desktop checks, additional Intel/ARM hardware coverage, an older low-memory computer, and a larger
independently annotated image set remain needed. Linux headless/WSL checks do not verify a native
Wayland/X11 desktop installation. No alternative models such as Moondream have been integrated or
benchmarked. No user source images or the original catalog are used or modified by these tests.

These model measurements were taken on the pre-split implementation. Model bytes and inference code are unchanged by the split. XMP, metadata parsing and reload are reviewed and validated separately.
