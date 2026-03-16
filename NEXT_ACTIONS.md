# NEXT_ACTIONS

This repository holds the repo-local execution lane for the QSL opaque encrypted attachment plane/runtime.

### NA-0001 — Attachment Service Runtime Implementation

Status: READY

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
