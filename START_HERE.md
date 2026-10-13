# Start here

Goals: G1, G2, G3, G4, G5

Revision: 2026-10-13. Read [the project workflow](PROJECT_WORKFLOW.md).

Follow PROJECT_WORKFLOW.md in full for all project work. The service stores opaque ciphertext, with scoped upload/fetch capabilities. No plaintext/key handling or secret-bearing canonical URLs. Single-root durability/backup limits remain explicit; do not imply unsupported hot/partial restore.

The managed current lane TASK is the single task authority; no repository-specific queue launches work. Manual, one task at a time, normal speed, Pushover USER_PAUSED. Preserve issued inputs and frozen snapshots; required CI, safety core and human merge authority remain.

Read README.md for component purpose, the active task/roadmap card for scope and relevant accepted contracts/source. Full historical decisions and testplan archives are read when needed for the actual claim, not mandatory startup corpus.

## Service authorization and durability

The deployment is the sole service policy subject. Quotas are deployment-global; resource references are not principals. Many transfers are permitted when deployment policy and quota allow them. Each `resume_token` and `fetch_capability` stays scoped to one session or object. See docs/NA-0007_authn_authz_policy_subject_contract.md for the technical contract.

The current durability boundary is one single local storage root: graceful same-root restart is in scope; cold full-root backup/restore plus matching service configuration is the only supported backup shape; hot/live backup and partial restore remain unsupported. Abrupt-crash and open-session recovery remains fail-closed with bounded operator cleanup rather than cross-file transactional durability.

Startup reconciles that root explicitly: only coherent open sessions and committed objects are re-exposed, orphaned local artifacts are discarded, and operator-visible recovery markers remain summary-only and secret-safe. See docs/NA-0009_durability_recovery_contract.md and tests/NA-0010A_durability_recovery_validation_evidence.md for the contract and recorded validation limits.
