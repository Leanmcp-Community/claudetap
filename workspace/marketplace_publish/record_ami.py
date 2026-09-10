#!/usr/bin/env python3
"""Write the us-east-1 AMI id produced by 00_prereqs.sh into config.json."""
import json
import sys
from pathlib import Path

cfg_path = Path(__file__).resolve().parent / "config.json"
cfg = json.loads(cfg_path.read_text())
cfg["ami_id"] = sys.argv[1]
cfg_path.write_text(json.dumps(cfg, indent=2) + "\n")
print(f"  config.json ami_id = {sys.argv[1]}")
