Status: Authoritative
Owner: qsl-attachments governance
Last-Updated: 2026-03-29

# NA-0009 Durability / Recovery Contract

Goals: G4, G5

## 1. Purpose and result

This document records `NA-0009`.

It freezes the current qsl-attachments durability / recovery contract without changing attachment-service runtime semantics.

Result:
- chosen result: `DRC0`
- closeout path implied by this document: `AV1`
- the durability boundary remains operator-scoped, single-node, and local-disk only
- the next blocker is implementing this frozen contract explicitly in qsl-attachments runtime/tests/docs, not another governance-only holding pattern and not a storage-backend redesign

## 2. Authoritative inputs reviewed

This contract is grounded by the current merged state of:
- qsl-protocol `NEXT_ACTIONS.md`, `TRACEABILITY.md`, and `DECISIONS.md`, especially `NA-0200A`, `NA-0201`, `NA-0201A`, `NA-0211`, `NA-0211A`, `NA-0212`, and `NA-0212A`
- qsl-protocol `docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`
- qsl-attachments `README.md`, `START_HERE.md`, `DECISIONS.md`, and `TRACEABILITY.md`
- qsl-attachments `docs/NA-0002_operational_hardening_contract.md`
- qsl-attachments `src/lib.rs`
- qsl-attachments `tests/service_contract.rs`
- qsl-attachments `tests/NA-0003_constrained_host_validation_evidence.md`
- qsl-attachments `tests/NA-0004_reference_deployment_validation_evidence.md`
- qsl-attachments `tests/NA-0005_stress_soak_chaos_evidence.md`
- qsl-server `README.md`, `TRACEABILITY.md`, `scripts/check_relay_compatibility.sh`, `scripts/verify_remote.sh`, and `tests/NA-0011_relay_compatibility_restore_evidence.md`

## 3. Current evidence-backed facts

### 3.1 Storage and topology actually in scope

- qsl-attachments is still the current single-node local-disk runtime.
- The storage root contains session directories and object directories only:
  - `sessions/<session_id>/session.json`
  - `sessions/<session_id>/parts/<part_index>.part`
  - `objects/<locator_ref>/object.json`
  - `objects/<locator_ref>/ciphertext.bin`
- qsl-attachments persists only opaque ciphertext bytes, non-secret resource references, lifecycle metadata, and hashed capability material.
- No external database, WAL, object store, distributed lock, or multi-node durability mechanism exists today.
- qsl-server remains transport-only and out of the attachment durability boundary.

### 3.2 What restart / recovery is already proved

- The constrained-host and reference-host evidence both proved bounded service restart on the same storage root without attachment-service semantic break.
- The constrained-host and reference-host evidence both proved interrupted upload then resume on the same storage root without exposing an incomplete object to the receiver.
- Those evidence lanes prove same-root restart/retry truthfulness under bounded operator-managed conditions.
- No merged evidence proves hot backup, restore from backup, multi-host replication, or crash-time reconciliation of partially persisted local state.

### 3.3 What persistence mechanics exist today

- Session metadata writes use `write_json_atomic(...)`: write a temporary file, then rename it into `session.json`.
- Staged ciphertext part writes use `write_bytes_atomic(...)`: write a temporary file, then rename it into the part path.
- Object metadata writes use `write_json_atomic(...)`: write a temporary file, then rename it into `object.json`.
- Commit writes `ciphertext.bin` directly, flushes that file handle, saves `object.json`, then removes the old session directory.
- Startup ensures the storage layout exists and now performs bounded reconciliation of the same storage root before serving requests.
- That reconciliation keeps only coherent open sessions whose `session.json` still matches the named staged part files, discards extra orphan staged artifacts, and re-exposes committed objects only when `object.json` and `ciphertext.bin` both survive with matching length.

### 3.4 What the current code does not prove

- The runtime does not fsync files or parent directories, so it does not prove sudden host-crash or power-loss durability for already-written bytes.
- The runtime does not provide a cross-file transaction covering:
  - staged part bytes plus `session.json`, or
  - `ciphertext.bin` plus `object.json` plus session-directory removal.
- A crash between staged-part rename and `session.json` save can leave a staged part present on disk but not counted in the session journal.
- A crash during commit can leave:
  - `ciphertext.bin` without `object.json`, or
  - a committed object on disk while the old session directory still exists.
- The startup reconciliation remains bounded and fail-closed: it discards incoherent sessions/objects rather than reconstructing missing journals, missing parts, or partial committed objects.
- No merged evidence proves that open sessions survive backup/restore, and no merged evidence proves that live/hot backups are safe.

## 4. Frozen contract

The following rules are now authoritative for qsl-attachments.

### 4.1 Durability boundary

- qsl-attachments MUST continue to treat one operator-managed local storage root on one node as the entire durability boundary.
- This contract MUST NOT be interpreted as cloud, object-store, replicated, or multi-node semantics.
- The supported recovery unit is the whole storage root, not an individual file or a best-effort subset copied from different moments in time.

### 4.2 Crash consistency promises

- qsl-attachments promises fail-closed crash behavior, not cross-file transactional durability.
- An incomplete upload is recoverable only when `session.json` and the staged part files it truthfully names are both present after restart.
- A committed object is recoverable only when both `object.json` and `ciphertext.bin` are present for the same `locator_ref`.
- The current contract does NOT promise that an in-progress upload survives abrupt crash or power loss.
- The current contract does NOT promise exactly-once commit promotion across abrupt crash or power loss.
- Orphan staged parts, orphan ciphertext bytes, or leftover session directories after an interrupted commit are recovery artifacts, not successful attachment delivery.

### 4.3 Restart / recovery promises

- Graceful service-process restart on the same storage root is in scope.
- After restart, previously committed objects whose `object.json` and `ciphertext.bin` both survived MUST remain retrievable through the canonical API.
- After restart, an incomplete upload MAY continue only when the local session journal and its already-counted staged parts survived coherently; otherwise the service MAY fail closed for that session and the operator MAY abort/recreate it.
- The service does not promise automatic reconstruction of missing parts, missing object bytes, or missing journals.
- Expiry/cleanup behavior remains the existing request-path sweep model unless a later implementation item adds stronger recovery automation explicitly under this contract.

### 4.4 Backup / restore promises

- The only supported backup/restore shape under this contract is a cold or quiesced full copy/snapshot of the entire storage root plus the matching operator-managed service configuration.
- Hot/live backup while mutations continue is unsupported.
- Partial restore of only sessions, only objects, only part files, or mixed-time snapshots is unsupported.
- Restored committed objects are in scope only when both `object.json` and `ciphertext.bin` are present for the same `locator_ref`.
- Restored open sessions are best-effort only and are not part of the guaranteed recovery objective for this contract.

### 4.5 Bounded operator recovery expectations

- Operators MAY stop the service and inspect the local storage root when crash ambiguity exists.
- Operators MAY discard orphan staged parts or incomplete session directories rather than attempt to reconstruct them.
- Operators MAY discard orphan object bytes or object records that are missing their paired file.
- Operators MAY preserve or re-expose only coherent committed objects backed by both `object.json` and `ciphertext.bin`.
- Operators are not expected to recover plaintext, decrypt contexts, resume tokens, or fetch capabilities from local state.
- Any later implementation automation MUST preserve opaque ciphertext-only handling, secret-safe surfaces, and fail-closed behavior while performing the same bounded recovery decisions.

## 5. Option set

| Option | Summary | Evidence result |
| --- | --- | --- |
| `DR0` | implementation of an explicit durability / recovery contract is next | chosen: the code, canonical docs, and merged restart/resume evidence are already strong enough to freeze the single-node local-disk durability boundary, same-root restart expectations, cold whole-root backup/restore boundary, and unsupported hot-backup / cross-file transactional claims without semantic invention |
| `DR1` | one smaller explicit contract gap still blocks implementation | rejected: the remaining uncertainty is not another missing design choice; the contract can already say that open-session crash survival and hot backup are unsupported, and that committed-object recovery requires the paired on-disk files |
| `DR2` | continued operator-scoped support is the truthful next posture | rejected: operator-scoped posture remains true, but keeping the lane open without freezing the contract would hide the real next blocker, which is implementation of deterministic crash/recovery handling under the already-evident local-disk boundary |

Why `DR0` wins:
- `DOC-ATT-002` already required explicit restart/recovery and backup/restore expectations before reference deployment could claim readiness.
- The current runtime code already exposes the real storage-root boundary and the exact non-transactional write ordering that matters for crash truthfulness.
- `NA-0200A`, `NA-0201`, and `NA-0201A` already proved bounded same-root restart and resume behavior strongly enough to distinguish restart truthfulness from backup/restore speculation.
- `NA-0211`, `NA-0211A`, `NA-0212`, and `NA-0212A` already froze the logging/secret-hygiene and operator-scoped policy boundary, so the remaining blocker is deterministic durability/recovery implementation rather than another governance-only lane.

## 6. Decision

Chosen result:
- `DRC0`

Exact reason:
- the current canonical docs, runtime code, and merged evidence are already decision-grade enough to freeze a truthful durability / recovery contract for the current single-node local-disk service without inventing distributed semantics, stronger crash guarantees, or online-backup promises that the code does not support

Exact remaining blocker:
- implement the frozen contract so crash windows, startup recovery handling, and cold whole-root backup/restore behavior become deterministic and test-backed rather than implicit, best-effort, or operator-manual only

Smallest truthful successor lane:
- `NA-0010 — Durability / Recovery Implementation`

## 7. References

- qsl-protocol `docs/canonical/DOC-CAN-005_QSP_Attachment_Descriptor_and_Control_Plane_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-006_QATT_Attachment_Service_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/canonical/DOC-CAN-007_QATT_Attachment_Encryption_Context_and_Part_Cipher_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-ATT-002_qsl-attachments_Deployment_and_Operational_Hardening_Contract_v0.1.0_DRAFT.md`
- qsl-protocol `docs/design/DOC-G5-004_Metadata_Leakage_Surface_Review_and_Logging_Contract_v0.1.0_DRAFT.md`
- `README.md`
- `START_HERE.md`
- `DECISIONS.md`
- `TRACEABILITY.md`
- `docs/NA-0002_operational_hardening_contract.md`
- `src/lib.rs`
- `tests/service_contract.rs`
- `tests/NA-0003_constrained_host_validation_evidence.md`
- `tests/NA-0004_reference_deployment_validation_evidence.md`
- `tests/NA-0005_stress_soak_chaos_evidence.md`
