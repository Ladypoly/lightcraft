"""Regenerate reference-scores.json from pinned local weights, without downloading anything.

Development only: Python, torch, transformers (reference generated with 4.57.1), numpy.
Usage: python crates/autotag/tools/reference.py /path/to/tag-models
"""
import json
import sys
from pathlib import Path

import numpy as np
import torch
from PIL import Image
from transformers import CLIPConfig, CLIPImageProcessor, CLIPModel


def reference(models: Path) -> dict:
    torch.set_num_threads(4)
    source = Path(__file__).resolve().parents[1] / "src"
    tokens = [list(map(int, line.split("\t")[1].split(",")))
              for line in (source / "vocabulary.tsv").read_text().splitlines()]
    assert all(ids[0] == 49406 and ids[-1] == 49407 and len(ids) < 77 for ids in tokens)
    ids = torch.tensor([seq + [49407] * (77 - len(seq)) for seq in tokens])
    y, x = np.mgrid[:224, :224]
    image = np.stack([(x * 3 + y) % 256, (y * 2) % 256, (x + y * 4) % 256], axis=-1).astype("uint8")
    pixels = CLIPImageProcessor()(images=image, return_tensors="pt")["pixel_values"]
    wide = np.tile(image, (1,2,1))
    wide_views = [wide, np.asarray(Image.fromarray(wide).resize((224,224), Image.Resampling.BICUBIC))]
    wide_pixels = CLIPImageProcessor()(images=wide_views, return_tensors="pt")["pixel_values"]
    output = {}
    for name, width, heads, depth, text_depth in [
        ("tinyclip-8m", 256, 4, 10, 3), ("tinyclip-39m", 512, 8, 12, 6)
    ]:
        common = dict(hidden_size=width, intermediate_size=width * 4,
                      num_attention_heads=heads, hidden_act="gelu", projection_dim=512)
        config = CLIPConfig(
            text_config=dict(**common, num_hidden_layers=text_depth),
            vision_config=dict(**common, num_hidden_layers=depth, patch_size=16),
            projection_dim=512,
        )
        model = CLIPModel.from_pretrained(models / name, config=config, local_files_only=True).eval()
        with torch.inference_mode():
            text = model.get_text_features(ids)
            visual = model.get_image_features(pixels)
            scores = (visual / visual.norm(dim=-1, keepdim=True)) @ (text / text.norm(dim=-1, keepdim=True)).T
            visual_wide = model.get_image_features(wide_pixels)
            wide_scores = (visual_wide / visual_wide.norm(dim=-1,keepdim=True)) @ (text / text.norm(dim=-1,keepdim=True)).T
        assert scores.shape == (1, len(tokens)) and torch.isfinite(scores).all()
        output[name] = scores[0].tolist()
        output[name+"-wide"] = wide_scores.max(dim=0).values.tolist()
    return output


if __name__ == "__main__":
    print(json.dumps(reference(Path(sys.argv[1]))))
