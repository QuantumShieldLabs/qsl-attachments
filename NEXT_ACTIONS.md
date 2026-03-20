# NEXT_ACTIONS

This repository holds the repo-local execution lane for the QSL opaque encrypted attachment plane/runtime.

### NA-0001 — Attachment Service Runtime Implementation

Status: DONE

Problem:
- The canonical attachment service contract is frozen, but the opaque encrypted attachment plane does not yet exist as a runtime implementation.

Scope:
- runtime/service implementation inside `QuantumShieldLabs/qsl-attachments/**`
- no qsl-protocol runtime changes
- no qsl-server changes
- no website changes

Must protect:
- no plaintext attachments on service surfaces
- no capability-like secrets in canonical URLs
- deterministic session/commit/resume/retrieval rejects
- qsl-server remains transport-only
- `DOC-CAN-005` and `DOC-CAN-006` remain authoritative

Deliverables:
1) implement the canonical service/session/object lifecycle
2) implement opaque encrypted part upload/download/commit/resume
3) implement quota/retention/expiry/abuse controls and deterministic errors
4) add runtime tests proving contract faithfulness

Acceptance:
1) runtime faithfully implements the canonical service contract
2) no secret-bearing URL or plaintext-service leakage occurs
3) queue/evidence are updated truthfully

Evidence:
- implementation PR: https://github.com/QuantumShieldLabs/qsl-attachments/pull/2
- merge SHA: `da7400119b2af7a96e635aa8ce6becb1d9931dc4`
- mergedAt: `2026-03-16T01:18:39Z`
- runtime/contract-faithfulness: single-node local-disk runtime now implements `DOC-CAN-006` session create/upload/status/commit/abort/object retrieval, valid single-range fetch, deterministic reject codes, JSON journal persistence, opaque ciphertext-only storage, and secret-bearing header carriage through `X-QATT-Resume-Token` / `X-QATT-Fetch-Capability`.
- settings baseline: `main` branch protection now exists and requires only the `rust` check with strict up-to-date enforcement; no rulesets were present before or after; `allow_auto_merge=false` remained unchanged.
- secret-safe evidence hygiene: tests prove audit logs exclude raw resume/fetch capabilities and plaintext metadata; canonical URL query-string carriage is rejected; no plaintext attachment handling was added on service surfaces.

### NA-0002 — Deployment / Operational Hardening Contract

Status: DONE

Problem:
- The single-node local-disk attachment runtime now exists, but the deployment/readiness contract needed before any default-path promotion or legacy deprecation is not yet frozen.

Scope:
- qsl-attachments docs/governance only for deployment/readiness ladder and operational contract definition
- no runtime code changes
- no qsl-protocol or qsl-server runtime changes
- no website changes

Must protect:
- no plaintext on service surfaces
- no capability-like secrets in canonical URLs
- constrained-host results must distinguish saturation from correctness failure
- qsl-server remains transport-only
- qsl-protocol canonical docs remain authoritative

Deliverables:
1) define the repo-local deployment / operational hardening contract and constrained-host validation ladder
2) align `README.md`, `START_HERE.md`, `NEXT_ACTIONS.md`, and `TRACEABILITY.md` with the current single-node local-disk posture and the next implementation step
3) make the repo-local next item explicit without changing runtime code

Acceptance:
1) repo-local operational contract is explicit enough to execute next
2) no runtime or workflow changes occur
3) queue/evidence are updated truthfully

Evidence:
- promotion PR: https://github.com/QuantumShieldLabs/qsl-attachments/pull/4
- implementation PR: https://github.com/QuantumShieldLabs/qsl-attachments/pull/5
- merge SHA: `45550eb325e96803ef6041a81f89098346757c93`
- mergedAt: `2026-03-17T12:08:33Z`
- operational-contract/readiness-ladder summary: repo-local docs now align with qsl-protocol `DOC-ATT-002`, state the current single-node local-disk posture truthfully, define deployment profiles plus the constrained-host validation ladder, and freeze the exact readiness categories and promotion/deprecation gates without changing attachment semantics.
- runtime-change note: no runtime code, dependency, or workflow changes occurred in `NA-0002`; the repo-local output is docs/governance only.

### NA-0003 — Operational Hardening Implementation + Constrained-Host Validation

Status: DONE

Problem:
- The deployment/operational hardening contract is now frozen, but the qsl-attachments runtime has not yet implemented the operational controls, deployment posture, or constrained-host real-world validation ladder needed for default-path promotion/deprecation decisions.

Scope:
- qsl-attachments runtime/ops/docs as needed for operational hardening and real-world validation
- no qsl-protocol runtime redesign
- no qsl-server work

Must protect:
- no plaintext on service surfaces
- no capability-like secrets in canonical URLs
- constrained-host results must distinguish saturation from correctness failure
- qsl-server remains transport-only

Deliverables:
1) implement the operational hardening/readiness controls defined by NA-0200
2) perform constrained-host real-world validation over deployed qsl-attachments + real relay
3) capture resource/load evidence and classify saturation vs correctness
4) identify any final blockers to default-path promotion / legacy deprecation

Acceptance:
1) readiness ladder is executed truthfully
2) runtime/ops evidence is recorded
3) queue/evidence updated truthfully

Evidence:
- implementation PR: https://github.com/QuantumShieldLabs/qsl-attachments/pull/7
- merge SHA: `2d69abd084dd8918a0092385a92fcf56a8a6748b`
- mergedAt: `2026-03-18T22:58:25Z`
- operational-hardening summary: the runtime now enforces a storage-headroom reserve before create/upload/commit mutations, emits operator-safe startup configuration logs, and raises the ciphertext ceiling just enough to carry the `100 MiB` target class truthfully after part-cipher overhead.
- constrained-host ladder summary: direct evidence now covers `< 4 MiB` legacy-path success, exact `4 MiB` legacy-path weak-relay saturation with fail-closed bounded retries, `> 4 MiB` missing-service reject, `> 4 MiB` service-backed success, `16 MiB` / `64 MiB` / `100 MiB` service-backed success, upload-resume success, direct service-restart success, direct API quota/session/object-expiry rejects, and limited concurrency success over the restored real relay plus deployed single-node service on `qsl`.
- saturation-vs-correctness summary: no qsl-attachments correctness failure was proven in the required ladder; the exact `4 MiB` queue-full result was bounded weak-relay saturation with the service path idle, while a stricter exploratory receive-abort composite exposed a client-side confirm issue outside the minimum required service ladder.
- secret-safe evidence hygiene: no raw resume tokens, fetch capabilities, relay bearer tokens, or vault passphrases were written into repo artifacts or the live qsl-attachments journal during this item.

### NA-0004 — Reference Deployment Validation + Promotion Gate Evidence

Status: DONE

Problem:
- The constrained-host lane is now grounded, but the project still lacks stronger reference-deployment evidence showing how the integrated message + attachment system behaves on a materially stronger host profile before any default-path promotion or legacy deprecation decision can be made honestly.

Scope:
- qsl-attachments runtime/ops/docs as needed for stronger reference deployment validation and promotion-gate evidence
- no qsl-server work
- no qsl-protocol semantic changes

Must protect:
- no plaintext on service surfaces
- no capability-like secrets in canonical URLs
- mixed message + attachment evidence must distinguish saturation from correctness failure honestly
- qsl-server remains transport-only

Deliverables:
1) establish a stronger reference deployment profile for qsl-attachments while preserving current semantics
2) execute the reference validation matrix across message-only, attachment-only, and mixed traffic over the real relay
3) capture resource/load/restart/soak evidence and classify bounded saturation, correctness failure, and deployment immaturity honestly
4) identify the exact remaining blocker to default-path promotion / legacy `<= 4 MiB` deprecation decisions

Acceptance:
1) stronger reference deployment evidence is recorded truthfully
2) mixed message + attachment validation is recorded truthfully
3) queue/evidence updated truthfully

Evidence:
- implementation PR: https://github.com/QuantumShieldLabs/qsl-attachments/pull/10
- merge SHA: `3d3f1b6591180763cda020a35b684713bc58cc2b`
- mergedAt: `2026-03-19T03:04:24Z`
- stronger reference deployment summary: `qatt` now serves as a reproducible materially stronger reference host than constrained-host `qsl`, with the install/update/verify path captured in `docs/NA-0004_reference_deployment_runbook.md`; the deployed service stayed on loopback behind Caddy TLS, preserved the single-node local-disk runtime posture, and did not require any new qsl-attachments runtime changes.
- mixed message + attachment validation summary: direct evidence now covers message-only relay traffic, service-backed `5 MiB` / `16 MiB` / `64 MiB` / `100 MiB` attachment runs, mixed `16 MiB` message + attachment traffic, upload interruption-resume, direct service restart, bounded concurrency with two parallel mixed peers, and a five-iteration mixed short soak over the real relay plus `qatt`.
- saturation-vs-correctness summary: no qsl-attachments correctness failure was proven on the stronger reference deployment; corrected `< 4 MiB` and exact `4 MiB` threshold reruns remained weak-relay / legacy-path bounded saturation (`timeout` and `relay_inbox_queue_full`) with `qatt` effectively idle, so the remaining blocker is broader mixed message + attachment stress/soak/chaos evidence rather than reference-host hardening.
- secret-safe evidence hygiene: the evidence bundle and `qatt` service/proxy journal scans found no raw bearer tokens, resume tokens, fetch capabilities, or secret-bearing canonical URLs, and no plaintext attachment content appeared on service surfaces.

### NA-0005 — Message + Attachment Stress / Soak / Chaos Validation

Status: READY

Problem:
- The stronger reference deployment is now grounded, but the project still lacks bounded kitchen-sink evidence showing how mixed message + attachment traffic behaves under broader concurrency, soak, restart/recovery, and weak-host versus stronger-reference-host comparison before any default-path promotion or legacy deprecation decision can be made honestly.

Scope:
- qsl-attachments runtime/ops/docs as needed for bounded stress/soak/chaos validation over weak-host and stronger reference-host profiles
- no qsl-server work
- no qsl-protocol semantic changes

Must protect:
- no plaintext on service surfaces
- no capability-like secrets in canonical URLs
- mixed message + attachment evidence must distinguish correctness failure, bounded saturation, and deployment immaturity honestly
- qsl-server remains transport-only

Deliverables:
1) execute the bounded stress/soak/chaos matrix across message-only, attachment-only, and mixed traffic over the real relay while using `qsl` as the weak-host baseline and `qatt` as the stronger reference host
2) capture CPU/memory/disk/retry/backpressure/latency/recovery evidence for concurrency ramps, bounded soak, and restart/recovery stages
3) classify every degraded or failing stage explicitly as correctness failure, bounded saturation/degradation, or deployment immaturity
4) identify the exact remaining blocker to default attachment-path promotion and legacy `<= 4 MiB` deprecation decisions

Acceptance:
1) mixed message + attachment stress/soak/chaos evidence is recorded truthfully
2) weak-host versus stronger-reference-host comparison is recorded truthfully
3) queue/evidence updated truthfully
