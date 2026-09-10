#!/usr/bin/env python3
"""Write the public S3 logo URL produced by 03_upload_logo.sh into config.json."""
import json
import sys
from pathlib import Path

cfg_path = Path(__file__).resolve().parent / "config.json"
cfg = json.loads(cfg_path.read_text())
cfg["logo_url"] = sys.argv[1]
cfg_path.write_text(json.dumps(cfg, indent=2) + "\n")
print(f"  config.json logo_url = {sys.argv[1]}")
