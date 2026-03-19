# NA-0004 Reference Deployment Runbook

Status: Recorded

Purpose:
- Record the strongest reference deployment path used for `NA-0201`.
- Make the `qatt` host install/update/verify flow reproducible without storing secrets.
- Capture the minimum operator actions and the concrete issues hit during first install.

Non-goals:
- No deployment automation.
- No qsl-server source changes.
- No attachment semantic changes.
- No plaintext attachment handling on service surfaces.

## Reference profile

Host:
- SSH alias: `qatt`
- Public name: `qatt.ddnsfree.com`
- Provisioned profile: materially stronger than constrained-host `qsl`
- Observed baseline during `NA-0201`:
  - `4` vCPU
  - `MemTotal: 16080392 kB`
  - `/` filesystem: `102888095744` bytes total, `100381609984` bytes available at first validation snapshot

Network / ingress:
- inbound `22/tcp`, `80/tcp`, and `443/tcp`
- `qsl-attachments` binds only to loopback `127.0.0.1:3000`
- Caddy terminates public TLS on `qatt.ddnsfree.com` and reverse-proxies to `127.0.0.1:3000`

Runtime posture preserved:
- single-node local-disk runtime only
- opaque ciphertext-only service surfaces
- no capability-like secrets in canonical URLs

## Installed layout

Binary:
- `/opt/qsl-attachments/bin/qsl-attachments`

Config:
- `/etc/qsl-attachments/qsl-attachments.env`

State:
- `/var/lib/qsl-attachments/data`
- `/var/log/qsl-attachments`

Service:
- systemd unit at `/etc/systemd/system/qsl-attachments.service`

Proxy:
- Caddy config at `/etc/caddy/Caddyfile`

## Local build and copy

Build the release binary from a clean `qsl-attachments` checkout:

```bash
cargo build --release --locked
sha256sum target/release/qsl-attachments
file target/release/qsl-attachments
```

Create the host directories:

```bash
ssh qatt 'sudo -n install -d -o ubuntu -g ubuntu /opt/qsl-attachments/bin /etc/qsl-attachments /var/lib/qsl-attachments/data /var/log/qsl-attachments'
```

Copy the binary to the host and verify the digest:

```bash
scp target/release/qsl-attachments qatt:/tmp/qsl-attachments
ssh qatt 'sudo -n mv /tmp/qsl-attachments /opt/qsl-attachments/bin/qsl-attachments && sudo -n chmod 0755 /opt/qsl-attachments/bin/qsl-attachments && sha256sum /opt/qsl-attachments/bin/qsl-attachments'
```

## Host packages

Install Caddy on Ubuntu 24.04:

```bash
ssh qatt 'sudo -n apt-get update && sudo -n apt-get install -y caddy'
```

No Rust toolchain is required on the host for this path because the binary is built locally and copied in.

## Runtime config

Install `/etc/qsl-attachments/qsl-attachments.env`:

```dotenv
QATT_BIND_ADDR=127.0.0.1:3000
QATT_STORAGE_ROOT=/var/lib/qsl-attachments/data
QATT_MAX_CIPHERTEXT_BYTES=105906176
QATT_STORAGE_RESERVE_BYTES=134217728
QATT_MAX_OPEN_SESSIONS=64
```

Install `/etc/systemd/system/qsl-attachments.service`:

```ini
[Unit]
Description=QSL opaque encrypted attachment service (qsl-attachments)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=ubuntu
Group=ubuntu
WorkingDirectory=/var/lib/qsl-attachments
EnvironmentFile=/etc/qsl-attachments/qsl-attachments.env
ExecStart=/opt/qsl-attachments/bin/qsl-attachments
Restart=on-failure
RestartSec=2
NoNewPrivileges=true
ProtectSystem=full
ProtectHome=true
ReadWritePaths=/var/lib/qsl-attachments/data

[Install]
WantedBy=multi-user.target
```

Install `/etc/caddy/Caddyfile`:

```caddy
qatt.ddnsfree.com {
    reverse_proxy 127.0.0.1:3000
}
```

## Enable and verify

```bash
ssh qatt 'sudo -n systemctl daemon-reload && sudo -n systemctl enable --now qsl-attachments caddy'
ssh qatt 'sudo -n systemctl is-active qsl-attachments caddy'
ssh qatt 'ss -ltnp | grep -E ":3000|:80 |:443 " || true'
```

Probe loopback and public TLS with a secret-safe schema error:

```bash
ssh qatt 'curl -sS -o /tmp/qatt_probe_body -w "%{http_code}\n" -H "content-type: application/json" --data "{}" http://127.0.0.1:3000/v1/attachments/sessions && cat /tmp/qatt_probe_body'
curl -sS -o /tmp/qatt_probe_tls_body -w "%{http_code}\n" -H 'content-type: application/json' --data '{}' https://qatt.ddnsfree.com/v1/attachments/sessions && cat /tmp/qatt_probe_tls_body
```

Expected result:
- status `422`
- body complains about missing required JSON fields
- no plaintext attachment content or secret-bearing service URL components are exposed

## Mandatory relay preflight before traffic validation

Before any reference traffic run, rerun the already-restored relay compatibility guard on `qsl`:

```bash
ssh qsl 'sudo -n BASE_URL=http://127.0.0.1:8080 PUBLIC_BASE_URL=https://qsl.ddnsfree.com /tmp/directive147-qsl-server-scripts/verify_remote.sh'
```

Required result:
- canonical loopback `204`
- canonical public `204`
- `QSL_RELAY_COMPAT_RESULT PASS code=canonical_ok`

## Issues encountered during first install

- `caddy` was not installed on the fresh Ubuntu host; the first install required `apt-get install -y caddy`.
- The remote host did not have `rg`; operator inspection used `grep` instead.
- ACME/TLS issuance succeeded only after `80/tcp` and `443/tcp` were open publicly.
- No host-side Rust toolchain was present; local build + binary copy was the smallest truthful install path.

## Update path for future installs

1. Rebuild the release binary locally.
2. Copy it to `/opt/qsl-attachments/bin/qsl-attachments`.
3. Keep `/etc/qsl-attachments/qsl-attachments.env`, the systemd unit, and the Caddy config under explicit operator review.
4. Restart `qsl-attachments` and rerun the loopback/public schema probes.
5. Rerun the qsl relay compatibility guard before any qsc traffic validation.
6. Record the exact deployed binary digest and service status in the evidence bundle.
