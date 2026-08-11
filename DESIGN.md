# akra-hookers dashboard

## Intent

The dashboard is a quiet local control room for recalling what each coding-agent
terminal was asked to do. It must feel spatial and editable rather than like an
analytics table.

## Visual system

- Background: `#101418`; elevated panels: `#182027`; borders: `#2a3742`.
- Primary ink: `#e8edf2`; muted ink: `#91a1ad`; Codex accent: `#72d0a4`.
- Typography: system sans for controls; monospace for project paths, session ids,
  and hook status.
- Canvas nodes use rounded 12px cards with concise prompt text and a provider chip.

## Layout

- A compact left rail establishes project context before controls: project filter,
  Inbox count, new/manage-project actions, then work-location setup and management.
- The main region is an infinite React Flow canvas with controls in the lower right.
- A right detail panel opens for the selected activity and never obscures navigation.
- On narrow screens the rail, canvas, assignment controls, and detail panel stack in
  that order. No surface overlays navigation or introduces horizontal scrolling.
- Settings is a modal panel listing provider state separately from historic activity.

## Project and origin hierarchy

- A project is a user-named logical context, not a directory path. Separate clones,
  linked worktrees, and folders may be connected to one project through their origins.
- Each origin is configured explicitly as dedicated or shared. Dedicated origins
  route all history to one project; shared origins place new work in Inbox unless a
  same-conversation future route exists.
- Inbox means unassigned shared work. Assignment is conservative: only selected
  activities move, and future conversation routing is a separate default-off choice.
- Session and turn identifiers are technical detail. They never substitute for the
  project name and remain collapsed in the detail panel.

## Information density

- Normal cards show project or Inbox context, conversation sequence, a three-line
  prompt summary, provider, and compact local time. They do not show paths or IDs.
- Detail shows the complete prompt, truthful captured/first-recorded timestamps and
  provenance, submitted and detected paths separately, copyable collapsed technical
  IDs, and the immutable oldest-first conversation timeline.
- Korean text keeps semantic word boundaries where spaces exist. Unbroken Korean,
  URLs, and mixed text may emergency-wrap without expanding their container.

## Interaction

- Dragging, connecting, or deleting canvas nodes changes only `canvas_nodes` and
  `canvas_edges`; it never deletes an activity event.
- Provider toggles describe future capture only. Existing canvas and activity data
  remain visible after a provider is disabled.
- Empty state explains that a configured provider will populate the canvas after
  its next submitted prompt.
- Every modal traps and restores focus. Setup, assignment, technical disclosure,
  copy, detail close, and canvas-card activation remain keyboard operable with a
  visible focus indicator.
