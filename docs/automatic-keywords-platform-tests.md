# Automatic tag platform validation

Validation date: 2026-10-19. Tested working tree; not a published CI run.

The same four hash-pinned models and six public/CC0 photo fixtures are used across systems.
Model inference, source checksums, metadata search, manual keyword separation, review decisions,
catalog reopening, current-view scope, download installation and headless tagging UI are checked.
Both CLIP models are compared with Transformers references and both YOLOX graphs with ONNX Runtime
references. Product inference uses pure Rust CPU code; Python is development tooling only.

## Coverage

- Windows x86-64: 32 focused CPU/model/catalog/engine/download/UI checks passed; native app smoke test passed.
- Linux x86-64: the same 32 focused checks passed on Ubuntu 24.04 under WSL2, with GPU disabled and a headless UI.
- macOS Apple Silicon and Intel: tagging-crate cross-compilation checks only.
- macOS runtime: pending an actual Mac or the added GitHub Actions macOS job.

The dedicated `.github/workflows/automatic-tags.yml` matrix runs real inference on Windows,
Ubuntu and macOS. It downloads and verifies every model before running tests, preventing silently
skipped model tests. The workflow is prepared locally; it has not been pushed or run on GitHub.

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

A real macOS run, Intel/ARM coverage beyond compilation, an older low-memory computer, and a larger
independently annotated image set remain needed. Linux headless/WSL checks do not verify a native
Wayland/X11 desktop installation. No alternative models such as Moondream have been integrated or
benchmarked. No user source images or the original catalog are used or modified by these tests.

## Separate AI XMP annotations (2026-10-19)

Windows and Linux passed the XMP suite (36 tests), sidecar suite (59 tests), metadata crate
suites (90 unit, 10 ComfyUI, 5 robustness), tagging UI (6 tests) and activity UI (22 tests).
The two engine filters overlap; these counts are per suite, not a distinct-test total.
New checks cover separate structured RDF annotations, finite/bounded values, alternative prefixes,
empty scans, source fingerprints, save/read/undo, fresh-library import/reopen, foreign-property
preservation, accepted provenance and manual-only keywords, and one right-sidebar progress bar
with working cancellation. Original image bytes remain unchanged.

The model CI matrix also watches metadata and shared detector-reader changes, runs ComfyUI,
XMP, sidecar, batch-reload and activity regression tests, and retains source-hash checks.
Three Windows and Linux batch-reload tests passed, using blocked probe workers to check responsive commands,
cancellation without catalog edits, source/library guards, and preserving late edits in one Undo step.
