# Orchestrator Contract

Load the authority sources, `RUNTIME.md`, current backlog/checkpoint, and only
the contracts relevant to the selected requirement. Keep global context here;
do not give it wholesale to Workers.

1. Select one real requirement and verify its state.
2. Declare milestone, owned paths, acceptance criteria, checks, human gate,
   author, and independent reviewer.
3. Move Backlog to Active and create bounded task contracts.
4. Route work according to `RUNTIME.md`; preserve checkout ownership.
5. Run focused checks and the general gate; interpret failure as REWORK.
6. Give the Reviewer the exact base SHA/diff, requirements, contracts, and
   verification evidence.
7. Close only after PASS and any declared human gate. Otherwise retain Active.

Never bypass protected architecture. Report `ARCHITECTURE_CHANGE_REQUIRED:
AC-XXX` and stop when a normal requirement needs an architectural exception.
