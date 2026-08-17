---
name: akra-hookers
description: A precision local activity map built as an oxide-black spatial studio.
colors:
  bg: "#0b1012"
  canvas: "#0d1417"
  panel: "#151d20"
  panel-raised: "#192327"
  rail: "#101719"
  detail: "#0f171a"
  border: "#283438"
  border-strong: "#46565d"
  text: "#e9eee8"
  muted: "#98a4a6"
  subtle: "#879496"
  accent: "#8fc7a1"
  accent-strong: "#a9dfb8"
  steel: "#86a0a8"
  warning: "#d2a65d"
  error: "#d57472"
typography:
  headline:
    fontFamily: '"Segoe UI Variable", "Segoe UI", Inter, ui-sans-serif, system-ui, sans-serif'
    fontSize: "18px"
    fontWeight: 620
  activity:
    fontFamily: '"Segoe UI Variable", "Segoe UI", Inter, ui-sans-serif, system-ui, sans-serif'
    fontSize: "16px"
    fontWeight: 570
    lineHeight: 1.44
    letterSpacing: "-0.012em"
  detail:
    fontFamily: '"Segoe UI Variable", "Segoe UI", Inter, ui-sans-serif, system-ui, sans-serif'
    fontSize: "17px"
    fontWeight: 570
    lineHeight: 1.45
    letterSpacing: "-0.014em"
  body:
    fontFamily: '"Segoe UI Variable", "Segoe UI", Inter, ui-sans-serif, system-ui, sans-serif'
    fontSize: "13px"
    lineHeight: 1.6
  label:
    fontFamily: '"Segoe UI Variable", "Segoe UI", Inter, ui-sans-serif, system-ui, sans-serif'
    fontSize: "11px"
    fontWeight: 650
    letterSpacing: "0.085em"
  mono:
    fontFamily: '"Cascadia Code", "SFMono-Regular", Consolas, ui-monospace, monospace'
    fontSize: "10px"
    fontWeight: 500
    lineHeight: 1.5
rounded:
  control: "5px"
  panel: "8px"
spacing:
  space-1: "4px"
  space-2: "8px"
  space-3: "12px"
  space-4: "16px"
  space-5: "24px"
components:
  button-primary:
    backgroundColor: "#5d956f"
    textColor: "#07120b"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "38px"
  button-secondary:
    backgroundColor: "#131b1e"
    textColor: "#c2cbc7"
    rounded: "{rounded.control}"
    padding: "7px 10px"
    height: "34px"
  button-danger:
    backgroundColor: "#7d3e3c"
    textColor: "#fff3f1"
    rounded: "{rounded.control}"
    padding: "8px 12px"
    height: "38px"
  field:
    backgroundColor: "#0d1416"
    textColor: "{colors.text}"
    rounded: "{rounded.control}"
    padding: "8px 10px"
    height: "38px"
  rail-item:
    backgroundColor: "transparent"
    textColor: "#bfc9c5"
    rounded: "0"
    padding: "8px 18px"
    height: "42px"
  capture-target:
    backgroundColor: "#0d1416"
    textColor: "{colors.steel}"
    rounded: "{rounded.control}"
    padding: "11px 10px"
  activity-card:
    backgroundColor: "#172124"
    textColor: "{colors.text}"
    typography: "{typography.activity}"
    rounded: "{rounded.control}"
    padding: "17px 18px 16px"
    width: "288px"
  timeline-turn:
    backgroundColor: "#16211f"
    textColor: "#c5cfca"
    padding: "12px 12px 14px"
    height: "96px"
  confirmation-panel:
    backgroundColor: "#282416"
    textColor: "#e6d79d"
    rounded: "{rounded.control}"
    padding: "11px"
---

# Design System: akra-hookers

## Overview

**Creative North Star: "Spatial Studio A"**

Spatial Studio A treats the localhost dashboard as a precision work surface: an oxide-black drafting field framed by matte instrument panels, mineral-white type, cold-steel provenance, lichen-green state, crisp rules, and small registration marks. It is compact and operational, but never collapses into a generic dark dashboard or analytics table.

The canvas is editable organization around immutable evidence. Moving, connecting, assigning, or clearing canvas records may alter placement, edges, and project organization; the captured activity, prompt, timestamps, paths, and oldest-first conversation remain trustworthy evidence. Depth is structural, motion is brief, and decorative glow is absent.

**Key Characteristics:**

- Oxide-black drafting field with a 48px line grid
- Matte, rule-bounded instrument surfaces
- Mineral-white hierarchy with cold-steel technical metadata
- Lichen-green reserved for healthy, selected, connected, and focused state
- Registration-corner activity plates instead of generic rounded cards
- Responsive regions that stack without horizontal page scrolling

## Colors

The palette stays near-black and mineral-neutral so sparse state color can carry operational meaning.

### Primary

- **Lichen State** (`colors.accent`): Healthy capture, active navigation rails, selected nodes, edge connections, project identity, and timeline selection.
- **Strong Focus Lichen** (`colors.accent-strong`): The visible keyboard-focus outline; it is brighter for access, not decoration.

### Secondary

- **Cold Steel Provenance** (`colors.steel`): Providers, paths, technical identifiers, icons, and low-emphasis origin context.

### Neutral

- **Oxide Black** (`colors.bg`): The application ground.
- **Drafting Field** (`colors.canvas`): The spatial canvas behind the 48px line grid.
- **Matte Instrument Panels** (`colors.panel`, `colors.panel-raised`, `colors.rail`, `colors.detail`): Controls, raised utilities, navigation, and the inspector remain tonally distinct without becoming glossy.
- **Crisp Rules** (`colors.border`, `colors.border-strong`): Section division, control boundaries, and higher-emphasis structural edges.
- **Mineral White** (`colors.text`): Primary readable content.
- **Muted and Subtle Ink** (`colors.muted`, `colors.subtle`): Secondary labels, counts, hints, and inactive status.

### Semantic

- **Instrument Amber** (`colors.warning`): Partial capture and confirmation-adjacent caution.
- **Error Red** (`colors.error`): Error state and destructive intent, always paired with text or a control label.

**The State Color Rule.** Lichen-green marks state and selection; it never becomes a broad decorative wash.

**The No Glow Rule.** Do not add neon bloom, gradient aura, or ambient color haze; selected state uses crisp rings, borders, and restrained shadow.

## Typography

**Display Font:** `typography.headline`
**Body Font:** `typography.body` with the same Segoe-led UI stack
**Label/Mono Font:** `typography.mono` for provenance, paths, times, counts, and compact instrumentation

**Character:** The Segoe-led UI face is direct and comfortably readable in Korean and mixed Latin text. Cascadia-led mono text supplies an instrument-readout voice without letting paths and identifiers dominate the interface.

### Hierarchy

- **Headline** (620, 18px): Dialog titles and the strongest compact surface headings.
- **Detail** (570, 17px, 1.45): The full selected prompt in the inspector.
- **Activity** (570, 16px, 1.44): Three-line prompt summaries on activity plates.
- **Body** (13px, 1.6): Explanatory and confirmation copy that must remain legible in constrained panels.
- **Label** (650, 11px, 0.085em, uppercase): Rail and instrument-section headings.
- **Mono** (500, 10px, 1.5): Paths, provider labels, timestamps, identifiers, and technical facts; observed readouts range from 9px to 11px where density requires it.

**The Evidence Voice Rule.** Use monospace for provenance and machine-readable facts, never as a substitute for the project name or prompt hierarchy.

## Layout

The desktop shell is a full-height instrument frame with a 64px command bar, a 258px navigation rail, a dominant minmax canvas, and—when an activity is open—a `minmax(330px, 370px)` inspector. The canvas toolbar is 52px high; activity plates are 288px wide; the React Flow field uses a 48px line grid. Spacing follows the implemented 4px, 8px, 12px, 16px, and 24px rhythm.

At 1180px, the detail layout compresses the rail to 224px and the inspector to `minmax(310px, 330px)`, while assignment controls collapse to one column. At 1040px, the command bar hides the work-location control when room is constrained. At 900px, a detail-open shell becomes command bar, rail, 720px canvas, then full-height inspector. At 720px, every shell uses that stacked order; the command bar becomes three rows (48px, 42px, 52px), the canvas keeps a 720px minimum with at least 520px for the field, and icon/canvas controls become 44px touch targets. At 430px, secondary canvas context is removed and spacing tightens without hiding primary actions. The document maintains a 320px minimum and never introduces horizontal page scrolling.

On narrow screens, content order remains navigation, canvas and its assignment dock, then detail. The inspector does not overlay navigation. Korean keeps semantic word boundaries when spaces exist; unbroken Korean, URLs, paths, and mixed text may emergency-wrap inside their container.

**The Fixed Frame Rule.** Preserve the rail/canvas/inspector relationship at desktop and the explicit stacked order below 900px; do not replace it with floating overlays.

## Elevation & Depth

The system is flat by default and uses tonal separation, one-pixel rules, and a small structural shadow vocabulary. Shadows identify a bar, control cluster, evidence plate, assignment dock, or modal as a distinct working layer; they never simulate emitted light.

### Shadow Vocabulary

- **Command bar** (`0 8px 24px rgb(0 0 0 / 18%)`): Separates the global command plane from the working regions.
- **Toggle thumb** (`0 2px 5px rgb(0 0 0 / 34%)`): Gives the physical switch thumb just enough lift.
- **Canvas controls** (`0 12px 30px rgb(0 0 0 / 26%)`): Keeps the compact control strip legible over the grid.
- **Empty state** (`0 18px 42px rgb(0 0 0 / 25%)`): Lifts the passive explanation from the field.
- **Activity plate** (`0 14px 34px rgb(0 0 0 / 24%)`): Distinguishes captured evidence from the drafting field.
- **Selected activity plate** (`0 0 0 2px var(--color-canvas), 0 0 0 3px var(--color-accent), 0 18px 38px rgb(0 0 0 / 30%)`): Combines a crisp separated selection ring with modest lift.
- **Assignment dock** (`0 16px 38px rgb(0 0 0 / 35%)`): Marks a contextual action layer without covering the canvas.
- **Dialog** (`0 28px 70px rgb(0 0 0 / 52%)`): Reserves the strongest depth for a modal decision.

**The Structural Depth Rule.** Add elevation only when a surface changes interaction layer; never add shadow merely to decorate a resting panel.

## Shapes

Controls use gently machined corners (`rounded.control`, 5px); panels, docks, dialogs, and passive empty states use the slightly broader panel corner (`rounded.panel`, 8px). Full pills are limited to capture switches, while circular forms are limited to status dots, connection handles, timeline points, and other true indicators.

Activity plates carry the signature geometry: four 10px by 1px and 1px by 10px registration corners inset 6px from the plate. One-pixel borders and dividers remain crisp; navigation rows stay square so the 2px active rail reads as alignment, not ornament.

**The Instrument Edge Rule.** Prefer precise rules and shallow radii; do not turn every container into a soft floating card.

## Components

### Buttons

- **Shape:** Compact controls use the control radius, with 34–38px minimum heights on desktop and 44px targets for mobile icon and canvas controls.
- **Primary:** `button-primary` is the lichen-filled submit action with dark ink and strong weight.
- **Secondary:** `button-secondary` is a dark, rule-bounded canvas or dialog action; hover strengthens the rule and surface tone where implemented.
- **Danger:** `button-danger` appears only after destructive intent is explicit and moves to the brighter red treatment on hover.
- **Focus / Disabled:** All buttons use a 2px strong-lichen outline with 2px offset; disabled controls retain their label and reduce opacity to 45%.

### Inputs / Fields

- **Style:** `field` is a dark inset field with a strong one-pixel border, control radius, 38px minimum height, and 8px by 10px padding.
- **Focus:** The global strong-lichen focus outline sits outside the field, preserving its measured border.
- **Error / Disabled:** Errors use the red rule-and-panel treatment with text and `role="alert"`; disabled controls keep semantic content visible at 45% opacity.

### Navigation

Rail rows are square, full-width, and 42px minimum height. Hover adds a subtle matte panel; active state uses the same surface plus a 2px lichen rail and lichen icon. Counts remain mono and muted. At narrow widths the rail becomes a normal document region rather than an overlay or drawer.

### Capture Targets

Detected installations form a compact ruled matrix. Each row combines a terminal icon, human label, small environment tag, switch, text status with a matching dot, and a truncated mono path. The master switch may expose a mixed state; disabled capture affects future collection and never removes historic activity.

### Contextual Prompt Summaries

The capture rail includes a compact default-off `문맥 기반 프롬프트 요약` control. Smart mode is an explicit future-capture policy, never a retroactive rewrite: it leaves hook targets, trust state, and historic evidence untouched. The helper makes the boundary visible—only a current projected request and, when needed, the immediately previous three-line result summary may be sent to Spark. Pending, ready, and failed states use terse textual status alongside color so the control remains understandable without color alone.

### Collection Destination

The collection destination lives directly below the capture master state and above
the per-installation matrix. At rest it is a one-line instrument readout: square
`LOCAL` or `REMOTE` tag, ellipsized mono endpoint, textual delivery status, and a
single Change action. The endpoint/token form is disclosed only while editing so it
does not turn the rail into a permanent settings card. A remote address exposes the
privacy scope in plain language and requires a password field; a stored token is
never revealed. Pending and delivery-error states use text plus amber/red status,
and changing destination is immediate without a hook-restart instruction.

### Activity Evidence Plates

The 288px activity plate is the signature component: 5px corners, registration marks, a lichen project label, conversation position, three-line prompt, ruled metadata footer, provider dot, timestamp, and connection handles. A `문맥 보강` badge appears only when the displayed request used the previous result summary; a compact pending or fallback marker keeps state truthful without changing geometry. Hover lifts by 1px; selection uses a crisp separated lichen ring. Keyboard focus uses the same 2px focus outline with a 5px offset, and Enter or Space opens the inspector.

### Log Curation Workspace

`로그 정리` is an explicit project-scoped, full-canvas workflow rather than a modal.
Its three visible stages are log selection, AI proposal review, and confirmation.
Selection rows lead with the compact request summary and one result line, keep exclude
and confirmed soft-delete controls separate, and end in a sticky summary-only action
dock. `오늘` is the browser-local calendar day while `24시간 동안` is a rolling
window, and the same boundary must drive canvas nodes, project/origin counts, and
curation logs. The unreviewed list exposes a native, indeterminate `전체 선택` bounded
to the same 20-log model limit. Destructive trash actions remain visibly red. A closed
`더보기` disclosure reveals the full stored request and the available three-line result
without making every row tall. Failed results show a compact asynchronous regeneration
action only while the original assistant result is still inside its bounded retention
window; unavailable historical results never imply that the original prompt can be
rerun. The review stage uses ruled columns for existing-work attachment and new-work
creation. Drag and a native select provide equivalent reassignment; editable titles,
confidence, and uncertainty remain visible. No work mutation happens until the final
apply action, and the workspace states that AI cannot delete evidence or create edges.

### Project Memory Work Plates

The work map is a separate mode from the raw activity log. A 320px work plate has a
lichen left rule, project and log-count provenance, a user-confirmed title, up to two
source-log previews, and a `사용자 확인` footer. It deliberately does not imitate an
individual prompt plate. New works appear without edges; connection handles create
only user-authored relationships and edge double-click removes only that relationship.
Removing a work returns its evidence logs to curation instead of deleting them.

### Work Evidence Inspector

The work inspector begins with the editable work identity and then gives the remaining
height to a scrollable source-log ledger. Each log shows its request summary, all three
stored result lines, a closed raw-evidence disclosure, an action to open the immutable
activity detail, and a reversible action to remove it from the work. The inspector must
never collapse evidence to make a long title or request fit.

### Detail Inspector

The inspector presents a request summary first, including whether it is contextual, current-request-only, deterministic projection, or fallback. When a derived request is shown, the raw captured prompt is available through a closed, independently scrollable `수집된 원문 보기` disclosure so long evidence does not collapse the conversation area. Project, provider, captured and first-recorded times, submitted and detected paths, collapsed technical IDs with copy actions, and an oldest-first timeline remain available. Timeline rows are compact keyboard-selectable `REQ`/`RES` previews; the selected turn opens its full request and three-line result above. Opening detail moves focus into the inspector; closing restores focus to the originating plate. Selected timeline turns use a muted lichen panel and lichen point, not a new card silhouette.

### Assignment Dock

The dock appears only for a meaningful selection. Shared-origin activities expose explicit destination choices and a separate default-off future-route option. Dedicated-origin selections show a guardrail and route the user to work-location management; no canvas action may imply deletion of captured activity.

### Dialogs and Confirmation

Dialogs trap focus, restore it on close, keep actions sticky when content scrolls, and reserve the strongest shadow for modal decisions. Clearing the canvas requires confirmation and removes only saved placement and connections. Project merge requires a second explicit confirmation state. Reconfiguring a populated work location requires an acknowledgement checkbox before save is enabled.

Motion stays concise: state colors and activity lift use 140ms transitions, switch travel uses 160ms with `cubic-bezier(.2, .8, .2, 1)`, and pending capture alone may pulse over 1.4s. Under `prefers-reduced-motion: reduce`, animation and transition duration becomes 0.01ms and iteration count becomes one.

## Do's and Don'ts

### Do:

- Do keep lichen-green scarce and tied to operational state, selection, focus, and connectivity.
- Do preserve the 48px drafting grid, registration corners, crisp rules, and matte panel hierarchy.
- Do keep captured evidence visually and behaviorally distinct from editable canvas placement, edges, and project organization.
- Do preserve visible focus, focus restoration, 44px narrow-screen targets, reduced-motion behavior, and semantic error text.
- Do let Korean, paths, URLs, and mixed strings wrap inside their own region without horizontal page scrolling.

### Don't:

- Don't add decorative glow, gradients, glass blur, glossy surfaces, or saturated ambient color.
- Don't expose session IDs, turn IDs, or full paths on ordinary activity plates or navigation rows.
- Don't turn capture switches, canvas deletion, assignment, or work-location changes into ambiguous history deletion.
- Don't overlay the inspector on navigation or reorder narrow screens away from rail, canvas/assignment, then detail.
- Don't replace the measured 5px/8px corner language with generic large-radius cards or pill-shaped containers.
