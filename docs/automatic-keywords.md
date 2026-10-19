# Automatic keywords

Open **Settings → Auto Tagging** to choose and download a model. Enable **Auto-tag new images**
there if you want missing images processed automatically while viewing a library, album or Local folder.
Downloads are explicit; photos are processed locally and never uploaded.

In **Keywords → Automatic Keywords**, a count such as **800 / 850 images tagged** shows coverage
for the current view, including its filters and subfolder setting. **Tag missing** processes only
unscanned or outdated images, without turning on automatic tagging. When the view is complete, the
same button becomes **Rescan**, which processes the current view again. A thin progress bar appears
while a scan runs, with **Cancel** in the same section. When this control is visible, tagging does
not also appear in the floating activity stack. If it is hidden, the shared activity stack provides
progress and Cancel instead. Images processed with no confident
suggestions still count as complete. Failed images remain missing and can be retried with **Tag missing**.

New photos increase the total as they enter the view. Turning on automatic tagging queues them;
with it switched off, press **Tag missing** when ready. Navigating elsewhere stops a one-time run.
Automatic tagging follows the new view. Dragging, typing and scrolling pause new image jobs.

| Model | Download | Recommended use |
|---|---:|---|
| TinyCLIP Balanced (default) | 332 MB | General libraries; better recognition, higher CPU and memory cost |
| TinyCLIP Compact | 94 MB | Smaller download and lower CPU/memory cost; noisier suggestions |

Both CLIP models run on the CPU in native Windows, macOS and Linux builds, without Python, CUDA or a cloud
service. See [platform validation](automatic-keywords-platform-tests.md) for the systems actually tested.
The browser can search saved tags but cannot download models or generate tags.

## Objects plus scenes

In **Settings → Auto Tagging → Object detector (YOLOX)**, choose **YOLOX Small** and download it
for more reliable common-object tags alongside CLIP. **Nano** is the faster, smaller alternative.
Choose **Off — CLIP only** to retain the existing CLIP vocabulary and behavior. Existing settings
remain CLIP-only until a detector is selected. The model manager uses the same explicit download,
progress, cancellation and retry controls as CLIP; both selected models must be installed to scan.

| Object model | Download | Input | Use |
|---|---:|---:|---|
| YOLOX Nano | 3.7 MB | 416 px | Lower CPU cost, weaker detection of small or obscured objects |
| YOLOX Small | 35.9 MB | 640 px | Better object recall; recommended starting point |

With a detector selected, YOLOX supplies objects and CLIP supplies scenes, activities and styles.
CLIP object guesses are excluded, even for object types YOLOX does not recognize. YOLOX recognizes
80 COCO classes, including cat, dog, chair, person, car, cup and table. This is not an arbitrary
object vocabulary, individual identity recognition, or a way to locate/count objects in the UI.
Common COCO names are normalized to **sofa**, **table** and **television**.

Each image is rendered once at up to 640 px and processed in the existing background queue.
YOLOX uses BGR values, aspect-preserving top-left letterboxing, confidence ≥ 0.35 and class-agnostic
NMS at IoU 0.45, capped to 300 candidates. Overlapping alternative classifications are suppressed
before one tag per class is stored. No boxes, full model outputs, or duplicate workflow data are
persisted. CLIP context scores are selected independently of CLIP's object guesses.

Changing or disabling the detector makes previous results eligible for **Tag missing**, using the
same coverage counter, progress bar and current-view scope. Manual and accepted tags and dismissals
survive. The combined model hashes, thresholds, source identity and index version are saved;
completed images do not run either model again on reopen. A missing or failed selected model
reports an error instead of silently marking CLIP-only results as hybrid results.

## Search and review

Type ordinary words such as `church`, `cat`, `coffee` or `beach` into Search Photos. Generated
keywords match immediately, alongside filenames, manual keywords and other indexed metadata.
Examples: `church rating:>3`, `tag:cat`, `keyword:beach`.
Search `cat chair` for images matching both words, or `tag:cat tag:chair` to require both keywords.
Add context with `cat chair indoors` or `person camera outdoors`. Words are combined with AND;
there is no need to type a literal `and`. Matching both objects does not establish their spatial relationship. Matching is case-insensitive; plain text
supports substrings and quoted phrases. Keyword filters keep their existing hierarchy semantics.
Listed keyword synonyms now participate in search and smart albums too.

AI suggestions appear separately from manual keywords, with an **AI** label. Click a suggestion
to filter by it. Hover to see its source model and score: object confidence for YOLOX, cosine similarity for CLIP (not a probability).
**Accept** makes an ordinary keyword, which follows the existing XMP/export settings; provenance
is retained in the catalog. **Dismiss** removes the suggestion from search and remembers the
decision across rescans and model changes. Both can be undone. Batch buttons act on the selection.
Indexing and dismissing tags never modify source files or automatically write sidecars.
Turning automatic generation off keeps existing tags searchable.

Generated tags are saved in catalog metadata separately from ordinary keywords. Find them in
**Info → All Metadata → Catalog · saved values → AI tags**, with their source models. Ordinary
keyword lists, suggestions, keyword counts and keyword-list exports show only manual and accepted
keywords. Existing accepted tags remain ordinary keywords; this change does not delete them.
Free-text searches, `tag:` / `keyword:` filters and smart-album text/keyword rules still include
current AI tags. Tree counts show manual/accepted assignments, while search also finds AI matches.
Ordinary keyword rename/delete actions leave unaccepted AI metadata alone. Accept a suggestion
before editing it through the keyword manager; accepted keywords retain their AI provenance.
Dismiss an unwanted prediction in the AI section to remember that decision across rescans.

## Saving AI tags beside images

Select one or more photos, then use **Photo → Save Metadata to File** (**Ctrl+S** on Windows/Linux,
**Cmd+S** on macOS). This writes the usual XMP sidecars beside the originals, including AI tags in
the separate **lc:aiTags** field. No acceptance is needed. Ordinary `dc:subject` and
`lr:hierarchicalSubject` keywords are not populated with suggestions. Existing sidecars are merged,
preserving other applications’ properties. Original image files are unchanged.

The structured RDF field stores each name, score, model identifier and acceptance state, plus dismissed
names, index identity and a source-file fingerprint. On import into a fresh LightCraft catalog, or
**Photo → Read Metadata from File**, matching files recover searchable AI tags without inference.
Copies and renames work; a sidecar for changed/replaced bytes does not restore stale predictions.
Empty completed scans also round-trip. Original-file export carries the same sidecar; rendered
JPEG/PNG/TIFF exports do not yet embed these annotations.

This uses LightCraft’s documented XMP namespace, not a universally supported AI-keyword standard.
Other applications and scripts can read the structured fields, but must support/map them to index
them separately. Apps that only read ordinary keywords will not search these AI tags. Use **Accept**
only when you want ordinary keywords for that broader compatibility. See the [XMP schema](xmp-interop.md#ai-tags).

## Reindexing and offline files

**Rescan** scans a fully processed view with the selected model again, retaining manual keywords, accepted
tags and dismissals. **Tag missing** leaves completed images alone. Switching models also queues a new scan. Records store the model digest,
vocabulary/index version, cutoff, source identity and review decisions. Completed images, including
those receiving no tags, resume without inference when the library reopens.

After replacing a source outside LightCraft, use **Photo → Reload from Disk**. Suggestions for
the old source become inactive and the background scan indexes the replacement. Missing or
unreadable images are reported, not marked successfully scanned; Tag missing retries them. Results
from a closed library or superseded source are discarded. Original images are never changed.

Catalog format 10 reads older catalogs and upgrades them on open. Older LightCraft builds refuse
the upgraded catalog, preventing them from silently dropping the new fields. Keep a library backup
if you need to return to an older build.

## Models and limitations

CLIP uses a fixed, original vocabulary of 222 English objects, scenes, activities and styles.
This is keyword suggestion, not arbitrary natural-language semantic search. Prompts and negative
prompts are not converted into visual tags. Faces, individual pets, specific vehicles, object
locations and counts are outside this feature; the existing People workflow handles named faces.
Multilingual vision encoders and specialist classifiers remain possible later work.

The MIT-licensed [Compact](https://huggingface.co/wkcn/TinyCLIP-ViT-8M-16-Text-3M-YFCC15M) and
[Balanced](https://huggingface.co/wkcn/TinyCLIP-ViT-39M-16-Text-19M-YFCC15M) weights are downloaded
at pinned revisions and checked for exact size and SHA-256 before installation and loading.
They are not bundled. See `assets/licenses/TinyCLIP-MIT.txt`. The pure-Rust Candle runtime is
already a workspace dependency. Model selection accepts only supported models, not arbitrary files.

The centre view is combined with a full-frame view for rectangular images, preserving edge subjects.
This costs up to two image inference passes; square images need one. Text embeddings are cached in
memory for the worker. Both model files stay unchanged and need no new download.

Labels qualify independently instead of having to sit within 0.025 of the top label. Selection uses
the model's minimum cosine score (0.34 Compact, 0.22 Balanced) and an adaptive floor of the image's
mean label response plus 2.5 standard deviations to reduce weak generic guesses. Scores are not
probabilities. Up to six subjects, two scenes, two activities and two styles can survive; no slot is
filled without qualifying evidence. Indoors/outdoors are not both assigned. Cup, mug, plate, bowl,
sofa, cushion, desk, keyboard, indoors and outdoors extend the vocabulary for secondary content.

Index version 2 makes earlier results eligible for **Tag missing** again. Automatic tagging processes
them if enabled. Manual keywords, accepted tags and remembered dismissals are preserved.

Six public-domain/CC0 photo fixtures cover two cat-and-chair compositions, cat-only and chair-only
negatives, coffee plus cup, and an outdoor photographer plus camera. Both models run these checks
when installed. This is a small regression set, not an accuracy benchmark. Balanced generally gave
cleaner suggestions in the smoke checks; Compact still confuses visually similar objects. Neither
model guarantees finding every small or obscured object, or distinguishing "on a chair" from
"next to a chair". Broad library evaluation remains needed; review suggestions when accuracy matters.

YOLOX uses the official Apache-2.0 [Nano and Small ONNX releases](https://github.com/Megvii-BaseDetection/YOLOX/blob/main/demo/ONNXRuntime/README.md),
with exact size/SHA-256 checks before both installation and loading. See
`assets/licenses/YOLOX-Apache-2.0.txt`. The bounded ONNX reader is reused from LightCraft’s face
runtime; inference uses the existing pure-Rust Candle dependency. The product needs no Python,
ONNX Runtime, CUDA, C/C++ library or cloud service. Only the two pinned graphs are supported.

On the six CC0 regression photos, Small finds both cat-and-chair compositions; Nano misses the
partially obscured chair. Both miss the ornate museum chair-only fixture, and Nano gives it a false
vase tag. This remains a small regression set, not an accuracy benchmark. Development tests compare
both networks' sampled raw outputs with independent ONNX Runtime references (maximum error < 0.001).
On this Windows machine, detector-only CPU inference took approximately 0.2 s for Nano and 0.55 s
for Small at native model resolution, before CLIP and decoding. A full library scan will take longer
than CLIP alone. GPU acceleration is not included. See [platform validation](automatic-keywords-platform-tests.md) for CPU tests and remaining platform coverage.

## Commands and verification

UI, CLI and MCP use `tags.models`, `tags.download`, `tags.cancelDownload`, `tags.configure`,
`tags.pump`, `tags.review`, and `tags.scan` (`rescan:true` to repeat completed images) or `tags.reindex`. `tags.configure` accepts `model`, `enabled`, `detector` (`yolox-nano`, `yolox-s`, or `null`), and
an optional cosine `minimum` in 0.18–0.5. A different model restores its recommended cutoff unless
an explicit cutoff is supplied. CLI/control clients call `tags.pump` to advance/collect jobs;
the desktop pumps automatically. Generation is one image at a time with bounded channels.

Set `LIGHTCRAFT_TAG_MODELS` to override `<config>/tag-models`. The library's own preferences hold
its opt-in setting and model choices. No model cache or raw workflow JSON is stored in catalog tags.

Run `cargo test -p lightcraft-autotag -p lightcraft-catalog -p lightcraft-engine` and the
`automatic_keyword_panel` UI test. Tests requiring downloaded weights record a Model skip unless
`LIGHTCRAFT_TAG_MODELS` is set. With both models present, the Rust implementation is compared to
Transformers reference scores on a procedural image (maximum error < 0.0002). Optional real-image
smoke checks use `LIGHTCRAFT_TAG_TEST_IMAGES` containing `chelsea.png`, `coffee.png`, `horse.png`
and `camera.png` from scikit-image; these CC0 images remain outside Git.

On this Windows development machine, optimized dev builds took about 55–60 ms per Compact image
and 130–150 ms per Balanced image, excluding decoding; loading and encoding the vocabulary took
about 2 and 8 seconds respectively. These are smoke timings, not cross-platform benchmarks.

The dedicated `Automatic tags` GitHub Actions workflow tests real downloaded models on Windows,
Linux and macOS with GPU use disabled. It validates all model sizes/digests before tests, so missing
weights cannot silently turn real-inference tests into passes. Locally, build
`cargo build -p lightcraft-autotag --example tagging-bench`, then use the example with a model id
to measure CPU load time and 18 inference samples on six public fixtures. This excludes decoding,
rendering, catalog/UI overhead and cold-disk effects. The dev-only downloader accepts the benchmark
executable path: `python crates/autotag/tools/download-models.py <path-to-tagging-bench>`.

Model-validation CI is proposed separately.
