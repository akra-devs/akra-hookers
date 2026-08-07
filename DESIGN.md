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

- A compact left rail contains the product mark, project selector, and Settings.
- The main region is an infinite React Flow canvas with controls in the lower right.
- A right detail panel opens for the selected activity and never obscures navigation.
- Settings is a modal panel listing provider state separately from historic activity.

## Interaction

- Dragging, connecting, or deleting canvas nodes changes only `canvas_nodes` and
  `canvas_edges`; it never deletes an activity event.
- Provider toggles describe future capture only. Existing canvas and activity data
  remain visible after a provider is disabled.
- Empty state explains that a configured provider will populate the canvas after
  its next submitted prompt.
