# DECISIONS

- **ID:** D-0001
  - **Status:** Accepted
  - **Date:** 2026-03-16
  - **Decision:** `QuantumShieldLabs/qsl-attachments` exists as the public repo-local runtime lane for the QSL opaque encrypted attachment plane. qsl-protocol owns the canonical control-plane and service-plane contracts; this repo will implement the runtime faithfully against those contracts.
  - **Invariants:**
    - no plaintext attachment handling on service surfaces
    - no capability-like secrets in canonical URLs
    - qsl-server remains separate and transport-only
    - `DOC-CAN-005` and `DOC-CAN-006` remain authoritative until explicitly superseded
  - **References:** qsl-protocol `DOC-CAN-005`; qsl-protocol `DOC-CAN-006`; `NEXT_ACTIONS.md`; `TRACEABILITY.md`
