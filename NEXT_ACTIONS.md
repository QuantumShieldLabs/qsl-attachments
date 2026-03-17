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

Status: READY

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
