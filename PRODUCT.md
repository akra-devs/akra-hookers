# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

Developers who use Codex across the Windows App, native CLI terminals, and WSL
worktrees and need to recover the intent and context behind local coding work.

## Product Purpose

akra-hookers records submitted Codex prompts and their work locations locally,
then lets the user recall, arrange, connect, inspect, and route those activity
records on a spatial canvas. Success means a developer can understand what was
asked, where it happened, and which project it belongs to without searching
through multiple terminal histories.

## Positioning

The product combines capture-time provenance with an editable spatial work map:
activity events remain immutable evidence while canvas placement, connections,
project assignment, and future routing remain user-controlled organization.

## Operating Context

- Codex prompts originate from the Windows App, native Windows CLI sessions, and
  one or more WSL distributions.
- Work happens across repositories, clones, linked worktrees, and shared or
  dedicated work locations.
- The dashboard is a localhost-only operational workspace used alongside coding
  sessions, not a cloud analytics product.
- Hook installation changes require restarting the affected Codex runtime and
  approving the hook through `/hooks`.

## Capabilities and Constraints

- Captures Codex `UserPromptSubmit` payloads into local spool and SQLite storage.
- Discovers Windows and WSL Codex homes and manages the akra hook independently
  per installation without removing unrelated hooks.
- Groups linked Git worktrees under a shared project identity.
- Supports project filtering, work-location setup and management, Inbox assignment,
  future routing, editable canvas positions, node connections, and detailed
  conversation history.
- Disabling future capture never removes historical activity.
- Binds to `127.0.0.1` by default and does not send telemetry, upload prompt data,
  or modify Git repositories.
- The interface must remain useful at desktop and narrow widths without horizontal
  page scrolling.

## Brand Commitments

- Product name: `akra-hookers`.
- Product language is direct, technical, and truthful about local state.
- Local-first privacy and clear separation between captured evidence and editable
  organization are durable commitments.

## Evidence on Hand

- Product and runtime behavior: `README.md`.
- Dashboard behavior and data contracts: `web/src`, `web/tests`.
- Capture, lifecycle, and API behavior: `crates/akra-app`,
  `crates/akra-adapters`, and their tests.
- No customer claims, benchmarks, pricing, or cloud-service claims are available
  and future work must not fabricate them.

## Product Principles

1. Preserve submitted activity as trustworthy local evidence.
2. Make cross-runtime provenance understandable without exposing technical noise
   by default.
3. Keep organization spatial, reversible, and explicitly controlled by the user.
4. Prefer local resilience and safe recovery over hidden automation.
5. Keep capture state and historical visibility clearly independent.

## Accessibility & Inclusion

Core capture controls, canvas activities, assignment actions, dialogs, disclosure
controls, and detail navigation must remain keyboard operable with visible focus.
Status and error states require semantic, non-color-only communication, and Korean
and mixed path text must wrap without breaking layout.
