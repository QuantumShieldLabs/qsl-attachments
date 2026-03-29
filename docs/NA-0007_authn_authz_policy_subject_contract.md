Status: Authoritative
Owner: qsl-attachments governance
Last-Updated: 2026-03-28

# NA-0007 Authn/Authz / Policy Subject Contract

Goals: G4, G5

## 1. Purpose and result

This document records `NA-0007`.

It freezes the current qsl-attachments authn/authz / policy-subject contract without changing attachment-service runtime semantics.

Result:
- chosen result: `ASC0`
- closeout path implied by this document: `AT1`
- current posture remains operator-scoped and single-node
- the next blocker is implementing this frozen contract explicitly in qsl-attachments runtime/tests/docs, not another governance-only holding pattern and not a multi-tenant auth redesign

## 2. Authoritative inputs reviewed

This contract is grounded by the current merged state of:
- qsl-protocol `NEXT_ACTIONS.md`, `TRACEABILITY.md`, and `DECISIONS.md`, especially `NA-0200A`, `NA-0201`, `NA-0201A`, `NA-0211`, and `NA-0211A`
- qsl-protocol `docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`
- qsl-attachments `README.md`, `START_HERE.md`, `DECISIONS.md`, and `TRACEABILITY.md`
- qsl-attachments `docs/NA-0002_operational_hardening_contract.md`
- qsl-attachments `tests/NA-0003_constrained_host_validation_evidence.md`
- qsl-attachments `tests/NA-0004_reference_deployment_validation_evidence.md`
- qsl-attachments `tests/NA-0005_stress_soak_chaos_evidence.md`
- qsl-attachments `src/lib.rs`
- qsl-attachments `tests/service_contract.rs`
- qsl-server `README.md`, `TRACEABILITY.md`, `scripts/check_relay_compatibility.sh`, `scripts/verify_remote.sh`, and `tests/NA-0011_relay_compatibility_restore_evidence.md`

## 3. Current evidence-backed facts

### 3.1 Deployment posture

- qsl-attachments is still the current single-node local-disk runtime.
- The validated deployment posture is operator-managed ingress/TLS around a loopback-bound service.
- qsl-server remains a separate transport-only relay boundary.
- qsl-attachments remains opaque ciphertext-only and does not parse plaintext attachment content or transcript metadata.

### 3.2 What authenticates and authorizes requests today

- `X-QATT-Resume-Token` authorizes status, upload, commit, and abort for exactly one `session_id`.
- `X-QATT-Fetch-Capability` authorizes retrieval for exactly one committed object addressed by one `locator_ref`.
- `Authorization` is explicitly reserved by `DOC-CAN-006` for a future repo-local layer and is not defined by the current canonical contract or current runtime.
- `session_id`, `locator_ref`, and `attachment_id` are non-secret resource references. They are not service principals and do not authorize actions by themselves.
- The intended sender/receiver identity remains in the authenticated qsl protocol message plane, not in qsl-attachments service-plane auth.

### 3.3 What policy subjects are real today

The current contract freezes exactly one service policy subject:
- the operator-scoped deployment subject

This subject is real today because it already owns:
- service configuration
- retention TTL configuration
- deployment-global quota ceilings
- deployment-global abuse ceilings
- ingress/TLS/auth placement around the service
- and operator-visible observability policy

The following are not current service policy subjects:
- the qsl message sender
- the qsl message receiver
- an `attachment_id` owner
- a `session_id` owner
- a `locator_ref` owner
- a per-user or per-device account inside qsl-attachments

`resume_token` and `fetch_capability` are current resource authorizers, not named service identities. Possession proves authority for one session/object operation only. It does not create a user/account/policy subject.

### 3.4 What quotas and limits are real today

The current runtime and evidence prove the following load-bearing limits:
- per-session byte ceiling: declared `ciphertext_len`
- deployment-global object-size ceiling: `QATT_MAX_CIPHERTEXT_BYTES`
- one active session per `attachment_id`
- deployment-global open-session ceiling: `QATT_MAX_OPEN_SESSIONS`
- retention/expiry TTLs keyed by `retention_class`
- bounded invalid-secret-attempt ceilings keyed by `session_id` and `locator_ref`
- bounded invalid-range-attempt ceilings keyed by `locator_ref`

Quota ownership under this contract:
- deployment-global ceilings belong to the operator-scoped deployment subject
- session/object shape checks belong to the specific resource being acted on
- no per-user, per-peer, or per-attachment-owner quota subject exists today

### 3.5 Legitimate metadata exposure under the current posture

Legitimate operator-visible metadata today is limited to:
- non-secret resource references needed for truthful service behavior: `session_id`, `locator_ref`, `attachment_id`
- ciphertext shape and lifecycle metadata: `ciphertext_len`, `part_size_class`, `part_count`, `retention_class`, expiry timestamps, state transitions, range usage
- deterministic reject/result metadata: canonical `reason_code`, bounded counters, coarse state
- passive-log correlation handles that are short, non-secret, and operator-scoped

Metadata that remains prohibited outside canonical carriage:
- `resume_token`
- `fetch_capability`
- any future secret-bearing auth material
- `enc_ctx_*`
- plaintext filenames/media types on service surfaces
- route tokens
- copied protocol payloads or long stable identifiers in passive logs/evidence when short handles are sufficient

## 4. Frozen contract

The following rules are now authoritative for qsl-attachments.

### 4.1 Policy-subject boundary

- qsl-attachments MUST treat the operator-scoped deployment as the sole current service policy subject.
- qsl-attachments MUST NOT infer or expose a separate end-user, peer, device, or attachment-owner service identity from `attachment_id`, `session_id`, `locator_ref`, IP address, or request timing.
- qsl-attachments MUST continue to treat qsl protocol peer identity as a message-plane concern outside the service auth boundary.

### 4.2 Authn/authz boundary

- Current request authorization is resource-scoped capability authorization plus deployment-scoped service policy.
- `resume_token` and `fetch_capability` MUST remain exact-match bearer capabilities for one session/object only.
- Those capabilities MUST NOT be renamed, documented, logged, or implemented as account identities.
- `Authorization` remains undefined in the current runtime contract.

If a later repo-local implementation introduces an explicit `Authorization` layer under this contract, it MUST:
- represent only the operator-scoped deployment subject or an equivalent deployment-local admin subject
- avoid inventing per-user or per-peer service identities
- keep qsl-server transport-only and qsl-attachments opaque ciphertext-only
- and avoid any new operator-visible metadata surface beyond what this contract explicitly allows

Any broader auth model needs a new contract item.

### 4.3 Quota ownership

- Deployment-global ceilings MUST be enforced as deployment-subject policy, not as per-user account quotas.
- Resource-local shape and integrity checks MUST remain tied to the specific session/object acted on.
- Abuse ceilings keyed by `session_id` or `locator_ref` are legitimate because they limit repeated misuse of one resource reference without inventing a new user identity.

### 4.4 Operator-visible identity

- The operator-scoped deployment subject remains implicit and out-of-band today.
- The service does not need to emit a new named `subject_id`, account name, or tenant id in order to satisfy this contract.
- Default passive logs and evidence MUST continue to prefer short deterministic handles over full stable identifiers.
- `NA-0008` may make the deployment subject explicit on operator surfaces by emitting an operator-safe policy summary rather than inventing a new end-user identity field.
- That operator-safe summary MUST preserve the frozen wording: `service_policy_subject=operator_scoped_deployment`, `quota_scope=deployment_global`, `authorization_model=deployment_policy_plus_resource_capability`, `authorization_header=reserved_undefined`, `resource_ref_model=resource_refs_not_principals`, and `transfer_model=many_transfers_subject_to_deployment_policy_quota`.

## 5. Option set

| Option | Summary | Evidence result |
| --- | --- | --- |
| `PS0` | implementation of an explicit contract is next | chosen: the current runtime/evidence already distinguishes the sole real policy subject, the current capability authorizers, and the real quota owners without needing another semantic invention |
| `PS1` | one smaller explicit contract gap still blocks implementation | rejected: no additional design input is needed to say that the current service is operator-scoped, capability-authorized, and not multi-tenant |
| `PS2` | continued operator-scoped support is the truthful next posture | rejected: operator-scoped posture remains true, but freezing it now removes the real ambiguity and makes implementation/testing of that contract the next blocker instead of more governance-only delay |

Why `PS0` wins:
- `DOC-CAN-006` already freezes the resource semantics and keeps `Authorization` reserved rather than defined
- qsl-attachments runtime/tests already prove that the live authorizers are per-resource capabilities and that the load-bearing limits are deployment-global
- the deployment/readiness evidence from `NA-0200A`, `NA-0201`, and `NA-0201A` is all operator-scoped and does not expose a hidden per-user subject
- `NA-0211` and `NA-0211A` already froze and enforced the metadata/logging boundary, so the remaining work is to implement this policy-subject contract explicitly rather than keep it implicit

## 6. Decision

Chosen result:
- `ASC0`

Exact reason:
- the current evidence is already decision-grade and unambiguous enough to freeze the real service auth boundary: one operator-scoped deployment subject, deployment-owned quotas, and resource-scoped capabilities that authorize session/object operations without creating service-account identities

Exact remaining blocker:
- make this contract explicit in qsl-attachments runtime/tests/docs/operator surfaces so quota ownership, policy-subject handling, and secret-safe observability remain truthful without inventing multi-tenant semantics

Smallest truthful successor lane:
- `NA-0008 — Authn/Authz / Policy Subject Implementation`

## 7. References

- qsl-protocol `docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`
- `docs/NA-0002_operational_hardening_contract.md`
- `tests/NA-0003_constrained_host_validation_evidence.md`
- `tests/NA-0004_reference_deployment_validation_evidence.md`
- `tests/NA-0005_stress_soak_chaos_evidence.md`
- `src/lib.rs`
- `tests/service_contract.rs`
