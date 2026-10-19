# Background Reload from Disk

Select photos and choose **Photo → Reload from Disk**. Desktop batches use the existing bounded import workers and the shared activity stack, with progress and Cancel. A single photo and CLI commands remain synchronous.

Cancellation before completion leaves the catalog unchanged. Completion preserves late manual edits, skips removed or changed photo records and commits one Undo step. Changing libraries drops old results. Unreadable files are reported without discarding saved metadata. Neither image files nor sidecars are written; the next ordinary edit still follows automatic-XMP preferences.

This changes neither the catalog format nor metadata parsers. ComfyUI indexing and automatic tagging are separate proposals.
