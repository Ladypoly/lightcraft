# ComfyUI metadata search

The library's existing **Search Photos** box searches metadata embedded in ComfyUI PNGs.
New imports are indexed automatically. Free text finds prompts, model and LoRA filenames,
node types, custom parameter names and values, strengths, and workflow descriptions.
Searches use the catalog and work even when originals are offline.

| Search | Matches |
|---|---|
| `comfy:true` | Images with a recognized ComfyUI graph |
| `comfy:false` | Images without indexed ComfyUI metadata |
| `prompt:cyberpunk` | Positive, negative or unclassified generation prompt text |
| `prompt:"neon city"` | A prompt containing this phrase |
| `positive:landscape negative:blurry` | Text on the corresponding conditioning branches |
| `model:flux` | Checkpoint, UNet or CLIP model filename |
| `lora:detail` | LoRA filename |
| `sampler:euler scheduler:karras` | Sampler and scheduler |
| `seed:123456` | Exact unsigned 64-bit seed, without floating-point rounding |
| `steps:>20 cfg:>5 denoise:<1` | Numeric generation parameters |
| `vae:ae node:custom` | VAE filename and node type |
| `width:1024 height:768` | Output or workflow dimensions |

Text fields accept case-insensitive substrings and double-quoted phrases. Numeric fields
accept `>`, `<`, `=` or an exact value. Every term must match, so these work together and
with existing filters, for example:

```text
comfy:true prompt:"neon city" model:flux lora:detail steps:>20 rating:>3
```

Quotes work with ordinary searches too: `"summer trip" camera:"Maker X2"`.
Within quotes, `\"` and `\\` represent a literal quote and backslash.
An unknown field retains its previous meaning as ordinary text, including the colon.
Existing smart albums using text filters use the same search path.

## Index an existing library or refresh replaced images

1. Select the PNGs in the grid (select all to index the current view).
2. Choose **Photo → Reload from Disk**.
3. Search using the same **Search Photos** box.

Reload works even when the file size and content hash have not changed, so it can index
images imported by older LightCraft versions. It replaces the generation index with the
current source's values, clears it when a replacement has no valid ComfyUI graph, and
refreshes virtual copies. Titles, keywords, ratings and develop edits are retained.
Unreadable or missing files retain their previous index; retry once the source is available.
The command returns per-file failures through `photo.reload` in the CLI/control/MCP interfaces.
Desktop batch reloads probe on the existing import workers, with progress and cancellation in
the shared activity stack. Cancel before completion leaves the catalog unchanged. Results for
removed, relinked or replaced records, or a different library, are discarded; edits made while
probing are kept. The completed reload is one Undo step. CLI/engine `photo.reload` remains synchronous.
Reload never writes the image or its sidecar, including with automatic XMP writing enabled. Probes use the existing import worker limits; ordinary searches perform no
file I/O, decompression or JSON parsing.

The catalog's optional metadata field is backward compatible with older catalog records.
Its journal and snapshots persist the compact index. Raw workflow JSON is never stored.

## Coverage and limits

Both API-format prompt graphs and UI-format workflows are supported; either text chunk can
appear alone. Compressed PNG text is supported. Standard widget layouts provide field names;
named inputs on custom nodes are recognized by parameter name, and other scalar values are
searchable as free text. Unknown positional custom widgets have no reliable field names, so
their values are available through free text. Positive/negative classification follows graph
connections where provided; disconnected prompt text is found by `prompt:`.

Malformed JSON, invalid CRCs, invalid compression and oversized fields are skipped. Extraction
has bounded text, traversal and index budgets; long values may be truncated and very large
workflows may be only partially indexed. See [the metadata crate](../crates/meta/README.md#comfyui-png-metadata)
for exact limits. Detection requires at least one node with a type and inputs (API), or a node
with an ID and type (UI); an arbitrary JSON object under a `prompt` key is not sufficient.


## View embedded metadata

Select an image and choose **Photo → All Metadata…**, or press **I**, scroll down the Info panel
and click **All Metadata…**. The existing viewer now includes **ComfyUI generation** (readable
fields with original case) and **PNG text / generator metadata** (raw embedded text). Search
fields and **Copy displayed metadata** work on these groups too.

All PNG text keywords are eligible, including `parameters` blocks used by Automatic1111 and
compatible generators, and InvokeAI metadata/graph keys. These are displayed as stored; their
format-specific normalization and fielded search are not implemented. Visual automatic keywords
work without embedded generation data; see [automatic keywords](automatic-keywords.md).

Text decoding checks chunk CRCs and bounds decompression: up to 32 text chunks, 4 MiB per chunk
and 8 MiB total. The raw viewer limits each row to 16 KiB and all raw rows to 64 KiB, with an
explicit truncation marker. It never edits the original PNG. This viewer is not a metadata editor.
