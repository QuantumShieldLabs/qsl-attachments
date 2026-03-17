# NA-0002 Operational Hardening Contract

Status: Draft

Purpose:
- Freeze the repo-local deployment / operational hardening contract for `qsl-attachments`.
- State the current runtime posture truthfully without changing runtime code.
- Make constrained hosts and weak relays, especially the AWS relay class, first-class validation inputs for the next implementation lane.

Non-goals:
- No runtime code changes.
- No deployment automation.
- No storage-backend redesign.
- No default-path promotion or legacy deprecation.
- No qsl-server changes.

## Current posture

- `qsl-attachments` is still the current single-node local-disk runtime.
- Opaque ciphertext parts and committed objects live on local disk.
- Session/object metadata is persisted in local JSON journals.
- `main` currently requires only the `rust` check.
- qsl-protocol remains authoritative for attachment semantics; this repo defines only repo-local operational execution requirements.

## Deployment profiles

### 1. Local development
- Intended use: developer bring-up, contract-faithfulness checks, local debugging.
- Storage assumption: enough local disk for one staged copy, one committed copy, and journal overhead for the largest local object in that run.
- Restart expectation: committed objects and journals survive a normal restart; abandoned staging may require manual cleanup.
- Ingress/TLS assumption: loopback or explicitly private ingress only.
- Observability level: redacted logs and manual inspection are sufficient.
- Mandatory stages: single-flow smoke, threshold ladder, basic reject/no-mutation checks.

### 2. Constrained-host validation
- Intended use: real-world validation on weak hardware and weak relay conditions.
- Storage assumption: enough durable free disk for one staged copy, one committed copy, journal headroom, and cleanup headroom for the active validation object class.
- Restart expectation: service restart and client restart are expected validation events; committed objects must survive restart.
- Ingress/TLS assumption: real network ingress with TLS before public exposure.
- Observability level: redacted logs plus explicit CPU, memory, disk, retry, backpressure, latency, and throughput capture.
- Mandatory stages: full single-flow ladder, threshold ladder, large-file ladder, interruption/resume ladder, expiry/quota/reject paths, secret-hygiene audit, resource observation; limited concurrency only after single-flow stages pass.

### 3. Reference deployment
- Intended use: stronger deployment used to separate constrained-host saturation from correctness failure.
- Storage assumption: durable storage sized for the largest supported object class, staging overhead, retention headroom, and backup/restore workspace.
- Restart expectation: restart, recovery, backup, and restore boundaries are explicit and repeatable.
- Ingress/TLS assumption: operator-managed TLS and ingress configuration are required.
- Observability level: redacted logs, metrics, and alert thresholds are required.
- Mandatory stages: full ladder including limited concurrency and repeated restart/recovery runs.

## Readiness categories

- Storage durability and recovery
- Retention, expiry, and cleanup
- Quota, abuse, and saturation handling
- Observability, metrics, and alerting
- Ingress, TLS, and secret handling
- Restart, resume, and interruption handling
- Resource and load characterization
- Rollout and promotion gates

## Constrained-host validation ladder

### Stage 1 — Single-flow smoke on deployed service + real relay
- Capture end-to-end success markers, wall-clock duration, CPU, memory, disk, and retry count.

### Stage 2 — Threshold ladder
- Validate `< 4 MiB`, `= 4 MiB`, and `> 4 MiB` behavior through the deployed stack.
- Confirm that only the above-threshold path depends on deployed `qsl-attachments`.

### Stage 3 — Large-file ladder
- Validate `16 MiB`, `64 MiB`, and the `100 MiB` target class when the host can sustain it honestly.

### Stage 4 — Interruption and resume
- Exercise client disconnect, service restart, and relay slowness / timeout / retry.

### Stage 5 — Expiry, quota, and reject paths
- Capture reject codes plus before/after journal/object state.

### Stage 6 — Secret-hygiene audit under real traffic
- Verify no plaintext attachment material, no resume/fetch capability leakage, and no secret-bearing canonical URLs.

### Stage 7 — Resource observation and saturation classification
- Record CPU, memory, disk, retry growth, backpressure behavior, latency, and throughput.

### Stage 8 — Limited concurrency
- Attempt only after Stages 1 through 7 pass in single-flow mode.

## Interpretation rules

- Slower throughput, higher latency, low concurrency ceilings, and bounded retries on constrained hardware are saturation signals, not automatic correctness failures.
- False `peer_confirmed`, secret leakage, silent state mutation, unbounded retry storms, or contract-breaking retrieval/integrity behavior are correctness failures.
- Stop immediately on any correctness failure.
- Stop escalation when the constrained host cannot sustain the next stage without unbounded resource growth.

## Promotion and deprecation gates

- Default-path promotion above threshold remains blocked until the constrained-host ladder is executed truthfully against deployed `qsl-attachments` plus the real relay, and at least one stronger reference deployment also separates saturation from correctness.
- Legacy `<= 4 MiB` deprecation remains blocked until default-path promotion is already justified, migration/rollback is explicit, and no silent break exists for legacy-sized flows.
- `NA-0002` does not authorize either move.

## Next repo-local step

The next truthful implementation item is:
- `NA-0003 — Operational Hardening Implementation + Constrained-Host Validation`

That item must implement the operational controls, execute the ladder, capture resource/load evidence, and classify saturation vs correctness honestly.
