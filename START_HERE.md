# START HERE

Read in this order:
1. `README.md`
2. `NEXT_ACTIONS.md`
3. `docs/NA-0007_authn_authz_policy_subject_contract.md`
4. `docs/NA-0002_operational_hardening_contract.md`
5. `tests/NA-0003_constrained_host_validation_evidence.md`
6. `docs/NA-0004_reference_deployment_runbook.md`
7. `tests/NA-0004_reference_deployment_validation_evidence.md`
8. `tests/NA-0005_stress_soak_chaos_evidence.md`
9. qsl-protocol canonical docs:
   - `DOC-CAN-005 — QSP Attachment Descriptor + Control-Plane Contract`
   - `DOC-CAN-006 — QATT Attachment Service Contract`
   - `DOC-CAN-007 — QATT Attachment Encryption Context and Part Cipher`
10. qsl-protocol design docs:
   - `DOC-ATT-002 — qsl-attachments Deployment and Operational Hardening Contract`

Canonical docs:
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md
- https://github.com/QuantumShieldLabs/qsl-protocol/blob/main/docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md

This repo is the runtime home for the opaque encrypted attachment plane.
It must not implement plaintext attachment handling or secret-bearing canonical URLs.
It must treat constrained hosts and weak relays as first-class validation inputs during operational hardening.
It now also carries the stronger reference-host install path, the `NA-0201` mixed validation evidence, and the bounded kitchen-sink stress/soak/chaos evidence for `NA-0201A`.
Its current service auth boundary is operator-scoped deployment policy plus per-session/object capability authorization, not a multi-tenant end-user identity model.
