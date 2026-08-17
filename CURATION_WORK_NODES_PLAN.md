# User-curated work nodes

## Goal

Turn immutable Codex conversation logs into a smaller, user-confirmed project memory.
A log is evidence; a work node is an editable interpretation of one or more logs. The
system may propose an interpretation, but it must not silently decide what the user
worked on.

## Product contract

- One log contains the captured user prompt, its compact contextual prompt summary,
  the matching assistant result summary, time, project, and provider provenance.
- A work node contains one or more logs from one project. V1 allows one primary work
  membership per log.
- A shared Codex session is only a weak grouping signal. A session can contain several
  unrelated tasks; project identity limits candidates but does not prove a match.
- Curation starts only from an explicit `로그 정리` action on a selected project.
- Creating an AI proposal does not mutate work items, memberships, logs, or edges.
- The user can rename groups, move logs between groups, split a group, return to the
  selection step, and then explicitly apply the reviewed proposal.
- New work nodes have no relationship edges. Only a user action on the work canvas
  creates or deletes an edge.
- Removing a log from a work returns it to `unreviewed`. Removing the last log removes
  the empty work. Removing a work returns all of its logs to `unreviewed`.
- `excluded` means “keep the evidence but omit it from normal curation.” Soft deletion
  is a separate, confirmed action that sets `deleted_at_us` and hides the activity from
  activity, project, conversation, canvas, summary-worker, and curation queries.
- Raw evidence remains available from the work inspector unless it was soft-deleted.

## Token-minimized AI boundary

One explicit proposal request uses `gpt-5.3-codex-spark` with low reasoning and the
same isolated `codex exec` runner used by result summarization. The request contains:

- at most 20 user-selected logs;
- the 96-character prompt summary or deterministic 96-character request preview;
- the stored three-line result summary, when ready;
- a small per-request session group number as a weak signal;
- at most five locally shortlisted existing works from the same project.

It does not contain the full raw prompt, raw assistant response, transcript, absolute
work path, Codex session ID, or turn ID. The compact JSON is capped at 64 KiB. The output
schema contains only target work ID, title, selected log IDs, confidence, and an
uncertainty flag.

A deterministic fingerprint covers the project, selected IDs, relevant summary
generations/digests, candidate works, and prompt version. An identical request reuses
its stored proposal without another model call. The model never receives permission
to delete logs or create work edges.

## Data model

- `work_items`: user-confirmed project-scoped work, title, canvas position, timestamps.
- `work_item_logs`: ordered evidence membership with a unique activity ID.
- `activity_curation_states`: explicit excluded state; absence means normal.
- `work_edges`: user-authored directed relationships between works in the same project.
- `work_state_revision`: polling revision for work, membership, and edge changes.
- `work_curation_proposals`: inert model output, fingerprint, selected log IDs, model,
  and application timestamp.
- `activity_events.deleted_at_us`: soft-deletion tombstone.

Proposal application is one transaction. It revalidates that every selected log still
exists, is visible, belongs to the proposal project, is not excluded, and is not already
organized. The reviewed grouping must cover the exact selected set once, with no
duplicates. A conflicting log-membership proposal fails without partial mutation.

## HTTP and UI flow

1. `GET /v1/curation/logs` lists project logs by `unreviewed`, `excluded`, or
   `organized` state.
2. `PATCH /v1/curation/logs/{id}` changes only the excluded state;
   `DELETE` performs the confirmed soft delete.
3. `POST /v1/curation/proposals` returns a cached or newly generated inert proposal.
4. The full-canvas review workspace lets the user edit the proposal without server
   mutation.
5. `POST /v1/curation/proposals/{id}/apply` persists the exact reviewed grouping.
6. The `작업 지도` renders confirmed work nodes. Its inspector exposes source logs;
   the React Flow handles create user-authored edges and double-click removes an edge.

The legacy `활동 로그` canvas remains available as a raw evidence view. This preserves
existing placement/inspection workflows while the work map becomes the higher-level
memory surface.

## Failure and privacy behavior

- No local Codex runtime, timeout, invalid model JSON, or failed validation leaves the
  selected logs unchanged and returns a retryable UI error.
- Proposal model output is validated for exact coverage, known candidate targets,
  nonblank bounded titles, finite confidence, and control characters before storage.
- Remote captures are summarized/curated only by the collector's trusted local Codex
  runtime; capture metadata may not select an executable or `CODEX_HOME`.
- API errors never return model output or captured evidence.

## Verification gate

- Migration upgrade/idempotence and soft-delete query propagation.
- Store tests for inert proposals, cache hits, exact apply, stale conflicts, weak session
  signals, cross-project rejection, edge CRUD, log/work removal, and soft deletion.
- Runner tests for compact prompt contents, isolation flags, output validation, and
  disabled model capabilities.
- API tests for authentication, nonmutation on AI failure, apply, work revision, edge
  lifecycle, exclude, and delete.
- Browser tests for summary-only selection, explicit confirmation, no automatic edge,
  evidence inspection, excluded-state restoration, responsive layout, keyboard focus,
  and existing dashboard regressions.
- Final gate: Rust fmt/clippy/workspace tests, TypeScript check, web unit/build, full
  Playwright suite, and `git diff --check`.
