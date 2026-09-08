## Optional cloud uploads

Basic cloud sync is implemented and tested. You can run your own server on your
laptop or a remote machine. Local capture continues independently of cloud
availability. Browser login, a web dashboard, managed storage, and marketplace
publishing remain follow-on work.

### Run your own server locally

Run these commands from the repository root. You need Rust/Cargo, Python 3, and
Docker Desktop (or Docker Engine with the Compose plugin).

1. Start Docker Desktop, then build and install the client:

   ```bash
   cargo install --path .
   ```

2. Create your organization upload key:

   ```bash
   mkdir -p deploy/secrets
   python3 server/create_key.py my-organization \
     --file deploy/secrets/keys.json
   ```

   Save the key printed by this command; it is shown once. The server key file
   contains its hash. The container runs as UID 10001, so that user must be able
   to read the file. On Linux, you can set ownership with
   `sudo chown 10001:10001 deploy/secrets/keys.json`.

3. Start the local server and check its health:

   ```bash
   docker compose -f deploy/compose.yaml up -d --build server
   curl http://127.0.0.1:8080/health
   ```

   The health check should return `{"status":"ok"}`. Captured data persists in a
   Docker volume. This setup exposes the server only on your own computer.

4. Connect the client:

   ```bash
   export CLAUDETAP_UPLOAD_KEY='paste-your-key-here'
   claudetap cloud configure --endpoint http://127.0.0.1:8080
   unset CLAUDETAP_UPLOAD_KEY
   ```

   This enables uploads for **new sessions**. To include existing captures, add
   `--backfill` to the configure command. Uploads include captured prompts,
   responses, and machine metadata. Structured credentials are filtered, but
   arbitrary content and binary bodies can still contain secrets.

5. Run the uploader in a separate terminal:

   ```bash
   claudetap cloud sync
   ```

   Continue using `claudetap` normally in another terminal. Check upload status
   with:

   ```bash
   claudetap cloud status
   ```

   For a single upload pass instead of a continuous worker, use
   `claudetap cloud sync --once`.

6. Optionally install the uploader as a background service:

   Stop the manually running uploader with Ctrl-C first, then run:

   ```bash
   python3 deploy/client-service.py install
   ```

   This installs a per-user macOS LaunchAgent or Linux systemd service. Remove
   it with:

   ```bash
   python3 deploy/client-service.py uninstall
   ```

### Stop uploads or the local server

Disable uploads while keeping local capture working:

```bash
claudetap cloud disable
```

An in-flight upload pass can finish. Stop the worker or uninstall its service
for immediate shutdown. Run `cloud configure` again to re-enable uploads.

Stop the local server without deleting its stored data:

```bash
docker compose -f deploy/compose.yaml stop server
```

### Deploy a remote server

Use `deploy/aws.sh` or `deploy/gcp.sh` to provision a VM. These scripts require
cloud credentials and the environment variables listed at the top of each
script; running them creates billable resources. They do not publish a
marketplace listing.

Follow [cloud setup and deployment](docs/CLOUD.md) to install Docker on the VM,
create its organization key file, configure DNS and firewall access, and start
it with HTTPS:

```bash
DOMAIN=capture.example.com ./deploy/start.sh
```

The same container setup works on other Linux Docker hosts. Configure clients
with the server's HTTPS URL and their upload key; clients do not need AWS or GCP
credentials. See the deployment guide for persistence, backup, retrieval API,
and operational limitations, and the [seven-part roadmap](docs/CLOUD_PLAN.md)
for remaining work.

### Validation status

The Rust build/test and four server and client/server integration tests passed.
Shell syntax and Docker Compose configuration were checked. Container runtime
validation was not completed because Docker's daemon was stopped during
implementation. AWS/GCP deployment scripts have not been exercised against a
cloud account.

Run `claudetap cloud --help` for the available uploader commands.

### Local management and analysis UI

After starting the server, open **http://127.0.0.1:8080/** and connect with your
organization upload key. The key is held only in the browser tab's memory.
The dashboard provides organization totals, paginated/filterable sessions,
file inspection, chunk downloads, request/error counts and mean request duration
for the displayed traffic chunk, and a server health check. These are captured
request metrics, not billing or token estimates. Refresh to see new uploads.
The UI cannot control the uploader process on another computer; use the CLI or
service installer for that. Browser identity login, user roles and key management
remain future work.

To apply UI updates to an already running local server:

```bash
docker compose -f deploy/compose.yaml up -d --build server
```

For automatic syncing, configure your key once as above, then run:

```bash
python3 deploy/client-service.py install
claudetap cloud status
```

The user service automatically starts the uploader during your login lifecycle.
On macOS, inspect it with `launchctl print gui/$(id -u)/com.claudetap.sync`.
