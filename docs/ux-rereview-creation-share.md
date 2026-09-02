**APPROVE**

Bounded re-review of the two user-visible fixes only (`3ba7c5a..857d98c`). Both are sound for a teacher and consistent with the already-approved B1–B3 intent.

**1. Root `index.html` titled "Actividad"**  
The card still shows name + type + **Abrir** / **Compartir**. The visible title is now **Actividad** (Spanish, classroom language) instead of the file stem `index`. That matches the type label **Actividad interactiva** and stops a filesystem name from leaking into default UX. Nested entries still use the parent folder (`actividad-2/index.html` → `actividad-2`); that was already the approved rule, not a new leak. Abrir/Compartir labels and `creation.id` targeting are unchanged.

**2. No duplicate cards on later turns**  
Turn 2 now registers only the sidecar-diff artifact, so the teacher sees one new card for the new activity, not a second copy of turn 1. The empty-diff scan still covers B1 when `/diff` reports nothing. That also restores B3: project-level Compartir’s latest-Web fallback can no longer promote a stale re-registration of the first activity.

**B1–B3:** Creation cards still expose Abrir and Compartir on the same registered artifact; public share still publishes that creation. These two fixes do not change the card actions or the share target.
