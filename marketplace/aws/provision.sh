#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get upgrade -y
apt-get install -y python3-venv caddy
id claudetap >/dev/null 2>&1 || useradd --system --home /var/lib/claudetap --shell /usr/sbin/nologin claudetap
python3 -m venv /opt/claudetap/venv
/opt/claudetap/venv/bin/pip install -r /opt/claudetap/requirements.txt
install -d -m 750 -o claudetap -g claudetap /var/lib/claudetap
install -d -m 750 -o root -g claudetap /etc/claudetap
cp /opt/claudetap/config.yaml /etc/claudetap/config.yaml
cat > /usr/local/sbin/claudetap-initialize <<'PY'
#!/opt/claudetap/venv/bin/python
import hashlib,json,secrets,os,pwd
from pathlib import Path
path=Path('/etc/claudetap/keys.json')
if not path.exists():
    os.umask(0o077)
    key=secrets.token_urlsafe(32)
    Path('/root/claudetap-upload-key').write_text(key+'\n')
    path.write_text(json.dumps({hashlib.sha256(key.encode()).hexdigest():'default'}))
    os.chown(path,0,pwd.getpwnam('claudetap').pw_gid)
    path.chmod(0o640)
PY
chmod 755 /usr/local/sbin/claudetap-initialize
cat > /etc/systemd/system/claudetap-initialize.service <<'UNIT'
[Unit]
Description=Initialize unique Claudetap credentials
Before=claudetap.service
[Service]
Type=oneshot
ExecStart=/usr/local/sbin/claudetap-initialize
RemainAfterExit=yes
UNIT
cat > /etc/systemd/system/claudetap.service <<'UNIT'
[Unit]
Description=Claudetap capture server
After=network.target claudetap-initialize.service
Requires=claudetap-initialize.service
[Service]
User=claudetap
Group=claudetap
WorkingDirectory=/opt/claudetap
Environment=CLAUDETAP_DATA=/var/lib/claudetap
Environment=CLAUDETAP_KEYS_FILE=/etc/claudetap/keys.json
Environment=CLAUDETAP_CONFIG=/etc/claudetap/config.yaml
ExecStart=/opt/claudetap/venv/bin/uvicorn app:app --host 127.0.0.1 --port 8080 --no-access-log
Restart=on-failure
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/claudetap
[Install]
WantedBy=multi-user.target
UNIT
cat > /usr/local/sbin/claudetap-https <<'PY'
#!/usr/bin/python3
import re,sys,subprocess
from pathlib import Path
if len(sys.argv)!=2 or not re.fullmatch(r'[a-zA-Z0-9](?:[a-zA-Z0-9.-]*[a-zA-Z0-9])?',sys.argv[1]):
    sys.exit('Usage: sudo claudetap-https capture.example.com')
Path('/etc/caddy/Caddyfile').write_text(sys.argv[1]+' {\n reverse_proxy 127.0.0.1:8080\n}\n')
subprocess.run(['systemctl','enable','--now','caddy'],check=True)
subprocess.run(['systemctl','reload','caddy'],check=True)
PY
chmod 755 /usr/local/sbin/claudetap-https
systemctl disable --now caddy
systemctl daemon-reload
systemctl enable claudetap
printf 'PasswordAuthentication no\nPermitRootLogin no\n' > /etc/ssh/sshd_config.d/00-claudetap.conf
# Do not initialize credentials on the builder. Each buyer generates unique keys.
touch /opt/claudetap/build-ready
