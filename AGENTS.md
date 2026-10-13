# Assistant entry

Goals: G1, G2, G3, G4, G5

Revision: 2026-10-13. Read [the project workflow](PROJECT_WORKFLOW.md).

Follow PROJECT_WORKFLOW.md in full for all project work. The service stores opaque ciphertext, with scoped upload/fetch capabilities. No plaintext/key handling or secret-bearing canonical URLs. Single-root durability/backup limits remain explicit; do not imply unsupported hot/partial restore.

The managed current lane TASK is the single task authority; no repository-specific queue launches work. Manual, one task at a time, normal speed, Pushover USER_PAUSED. Preserve issued inputs and frozen snapshots; required CI, safety core and human merge authority remain.
