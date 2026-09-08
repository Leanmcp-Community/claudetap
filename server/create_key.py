"""Append a revocable key hash to a server key file; print the new key once."""
import argparse, hashlib, json, os, secrets
from pathlib import Path
p = argparse.ArgumentParser()
p.add_argument('organization')
p.add_argument('--file', default='keys.json')
a = p.parse_args()
path = Path(a.file)
keys = json.loads(path.read_text()) if path.exists() else {}
key = secrets.token_urlsafe(32)
keys[hashlib.sha256(key.encode()).hexdigest()] = a.organization
fd = os.open(str(path) + '.tmp', os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
with os.fdopen(fd, 'w') as f:
    json.dump(keys, f, indent=2)
os.replace(str(path) + '.tmp', path)
print(key)
