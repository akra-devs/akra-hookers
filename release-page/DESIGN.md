---
name: Akra Hookers release page
description: An editorial black product exhibition built around real Akra Hookers evidence.
colors:
  exhibition-black: "#08080b"
  mineral-white: "#f7f6fa"
  editorial-muted: "#aaa8b4"
  hairline: "rgba(255,255,255,.12)"
  akra-blue: "#3e82ff"
  signal-cyan: "#78dcff"
  product-lichen: "#8fc7a1"
typography:
  display:
    fontFamily: '"Playfair Display", serif'
    fontSize: "clamp(4rem, 7.2vw, 7.4rem)"
    fontWeight: 500
    lineHeight: 0.78
    letterSpacing: "-0.035em"
  statement:
    fontFamily: '"Noto Serif KR", serif'
    fontSize: "clamp(2.1rem, 3.5vw, 4rem)"
    fontWeight: 600
    lineHeight: 1.22
    letterSpacing: "-0.035em"
  section-heading:
    fontFamily: '"Noto Serif KR", serif'
    fontSize: "clamp(2.6rem, 5vw, 5.6rem)"
    fontWeight: 600
    lineHeight: 1.15
    letterSpacing: "-0.04em"
  body:
    fontFamily: 'Inter, Pretendard, "Segoe UI", sans-serif'
    fontSize: "16px"
    lineHeight: 1.85
  label:
    fontFamily: "ui-monospace, monospace"
    fontSize: "10px"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "0.13em"
rounded:
  detail: "8px"
  panel: "14px"
  media: "22px"
  exhibition: "28px"
  pill: "999px"
spacing:
  compact: "12px"
  standard: "24px"
  section: "48px"
components:
  button-primary:
    backgroundColor: "#ffffff"
    textColor: "#09090b"
    rounded: "{rounded.pill}"
    padding: "0 24px"
    height: "50px"
  button-secondary:
    backgroundColor: "rgba(255,255,255,.05)"
    textColor: "{colors.mineral-white}"
    rounded: "{rounded.pill}"
    padding: "0 24px"
    height: "50px"
  screenshot-frame:
    backgroundColor: "#091012"
    rounded: "{rounded.media}"
    width: "min(100%, 760px)"
---

# Design System: Akra Hookers Release Page

## Overview

**Creative North Star: "Akra Product Exhibition"**

The release page follows the Akra home and Waxball exhibition language: an editorial black field, large serif statements, sparse blue light, and a product artifact that carries the first viewport. Visitors should understand the product mechanism before they reach the download section.

The page uses the real `product-canvas.png` dashboard capture as evidence. The image shows project nodes, prompt connections, capture state, and the detail inspector. A second product-tour sequence uses deterministic crops from current Playwright evidence to explain navigation, log curation, the confirmed work map, and work evidence without reconstructing the UI in HTML. Keep every capture legible and dominant. Synthetic browser chrome, invented metrics, customer claims, and substitute illustrations weaken the release case.

**Key Characteristics:**

- Editorial black exhibition field with restrained blue and violet illumination
- Playfair Display for the Akra product name and Noto Serif KR for Korean statements
- Real dashboard evidence framed as the hero artifact
- Product story ordered as context mechanism, guided product use, portable storage, then release
- GitHub release data used as progressive enhancement

## Colors

The palette joins Akra blue light to Hookers lichen state without copying the dashboard chrome into the marketing page.

### Primary

- **Akra Blue** (`colors.akra-blue`): Brand mark and blue-light atmosphere.
- **Signal Cyan** (`colors.signal-cyan`): Sequence labels, bullets, version text, and keyboard focus.

### Secondary

- **Product Lichen** (`colors.product-lichen`): Product-state references inside the screenshot world and portable-data boundary.

### Neutral

- **Exhibition Black** (`colors.exhibition-black`): Page ground and mobile navigation glass.
- **Mineral White** (`colors.mineral-white`): Display type and primary copy.
- **Editorial Muted** (`colors.editorial-muted`): Supporting copy and secondary navigation.
- **Hairline** (`colors.hairline`): Section dividers, secondary controls, and exhibition frames.

**The Evidence Color Rule.** Cyan explains the release page. Lichen belongs to product state and product-adjacent proof.

**The Dark Field Rule.** Keep white and spectral color sparse enough for the screenshot and download actions to lead.

## Typography

**Display Font:** Playfair Display with serif fallback
**Body Font:** Inter, Pretendard, Segoe UI, sans-serif
**Korean Editorial Font:** Noto Serif KR with serif fallback
**Label Font:** UI monospace with monospace fallback

Playfair gives the product name an Akra exhibition voice. Noto Serif KR carries Korean declarations and section headings. The sans stack handles explanations and controls; mono labels identify states, versions, files, and system facts.

### Hierarchy

- **Display** (500, `clamp(4rem, 7.2vw, 7.4rem)`, 0.78): `AKRA HOOKERS` in the hero.
- **Statement** (600, `clamp(2.1rem, 3.5vw, 4rem)`, 1.22): The Korean hero proposition.
- **Section heading** (600, `clamp(2.6rem, 5vw, 5.6rem)`, 1.15): Mechanism, portable, and download declarations.
- **Body** (16px, 1.85): Product explanation with a 39rem hero measure.
- **Label** (800, 10px, 0.13em): Platform facts and technical exhibition labels.

**The Serif Authority Rule.** Use serif type for product declarations and artifact names. Keep controls, facts, and explanations in sans or mono.

## Layout

The navigation sits 24px from the top and spans `min(1180px, calc(100% - 40px))`. The hero fills at least one small viewport height and uses two columns: `minmax(360px, .83fr)` for the proposition and `minmax(560px, 1.17fr)` for the screenshot. The page centers this grid within a 1280px content frame. The screenshot frame caps at 760px and uses a 1.6 aspect ratio with `object-fit: cover`.

Content sections use `min(1180px, calc(100% - 48px))`. The mechanism heading pairs flexible title space with a 33rem explanation. Its three-step sequence uses equal story columns connected by two 80px signal lines. The product walkthrough is a four-beat editorial sequence separated by hairlines; wide and portrait captures alternate with explanation rather than becoming a same-size card grid. The portable section uses a 1.12fr visual column and a .88fr copy column. Section spacing ranges from 100px to 170px so each product claim gets its own viewport beat.

At 980px, the hero, mechanism heading, sequence, and portable section become one column. The screenshot expands to `min(820px, 100%)`; portable copy moves before the folder visual. At 680px, navigation shrinks to `calc(100% - 24px)`, hides the wordmark text and center links, and keeps the download control. Hero actions become full-width, the screenshot loses perspective rotation, media corners tighten to 14px, content shells use 16px side margins, and the footer wraps.

**The First Viewport Rule.** Keep the proposition and real screenshot together on wide screens. On narrow screens, place the screenshot after the proposition and download actions.

## Elevation & Depth

The page combines a flat editorial field with two spectral radial gradients in the body background. The screenshot stage adds a blurred blue-violet aurora and a perspective tilt on wide screens. The folder exhibit uses a blue-black diagonal surface. These effects frame product evidence; text and ordinary sections stay flat.

### Shadow Vocabulary

- **Primary action** (`0 16px 42px rgba(255,255,255,.11)`): Separates the white download pill from the dark field.
- **Screenshot stage** (`0 36px 90px rgba(0,0,0,.52), 0 0 70px rgba(75,129,255,.1)`): Gives the real dashboard capture exhibition depth.
- **Portable folder** (`0 35px 90px rgba(0,0,0,.38)`): Lifts the storage model from its section.

Buttons move up 2px over 180ms. The stage aurora breathes over 5s. `prefers-reduced-motion: reduce` disables smooth scrolling, the aurora animation, and button transitions.

**The Artifact Depth Rule.** Reserve large shadow and spectral light for the screenshot and portable-data artifact.

## Shapes

Navigation and call-to-action controls use full pills (`rounded.pill`). The screenshot uses a 22px desktop frame and a 14px phone frame. The portable exhibit uses a 28px outer shell, a 14px folder body, a 16px tab, and an 8px data boundary.

Use one-pixel translucent borders around media, secondary actions, the folder exhibit, and section boundaries. The screenshot retains its own square dashboard geometry inside the rounded exhibition frame.

## Components

### Navigation

The wide layout shows the Akra mark, product links, GitHub, and a white download pill. The phone layout keeps the mark and download action, hides secondary links, and adds a dark translucent background with 14px backdrop blur. Navigation remains an absolute hero element rather than a sticky bar.

### Buttons

- **Primary:** White pill, dark ink, 50px minimum height, 24px horizontal padding, and 800 weight. Hover changes the surface to pale blue and lifts 2px.
- **Secondary:** Translucent white surface with a hairline border. Hover strengthens both.
- **Large download:** 58px minimum height and 30px horizontal padding.
- **Focus:** A 3px signal-cyan outline with 4px offset applies to buttons and navigation, scroll, and footer links.

### Product Screenshot

Use `product-canvas.png` as the hero evidence. Its alt text names the visible project nodes, connections, and activity detail. A caption identifies it as the real Akra Hookers product screen. Keep the image's center crop and do not place marketing copy over it.

### Context Sequence

Three articles explain `USER`, `RESULT`, and `NEXT`. Mono cyan labels introduce each step; Noto Serif KR titles carry the mechanism; signal lines connect the steps on wide screens. The 980px layout stacks the sequence and turns each connector into a short horizontal line.

### Product Walkthrough

Four actual-product crops explain the operating path: the left navigation rail, log curation, the confirmed work map, and the right evidence detail. Each beat pairs one capture with a plain-language role, a short explanation, and three exact UI concepts. Alternating media order gives the desktop sequence rhythm; below 980px every beat becomes capture then explanation. Portrait captures sit in restrained blue-light fields while wide captures keep their native proportions.

`product-rail.png`, `product-curation.png`, `product-workspace.png`, and `product-detail.png` are crops of current Playwright evidence. Do not redraw, annotate over, or fill them with invented content. Refresh the source evidence and regenerate all crops when the dashboard information architecture changes. The curation crop must stop before any raw request evidence so public release assets never normalize publishing captured prompt content.

### Portable Folder

The folder exhibit contrasts the movable `Akra Hookers.exe` artifact with the stable `%LOCALAPPDATA%\akra-hookers` user-data root. SQLite, spool, settings, and the stable sidecar bin appear as a mono file tree. It must not imply that captured data lives beside the executable: official portable and CLI builds intentionally share the Windows local-app-data store. Keep this component diagrammatic because it explains the storage contract rather than simulating a file manager.

### Release Download and Fallback

Static HTML points every release download to `https://github.com/akra-devs/akra-hookers/releases/latest`, so the action works before JavaScript and after API failure. JavaScript requests the GitHub latest-release endpoint and looks for `Akra-Hookers-Windows-x64-portable.zip`. A successful response replaces the fallback URL with the asset URL, adds tag, date, and size, and links the first `.sha256` asset when present.

API failure keeps the latest-release link active. The page replaces loading text with a clear metadata warning and confirms that the download path remains available. `#release-status` uses `role="status"` and `aria-live="polite"`, so assistive technology receives the update without a focus jump.

### Footer

The footer uses compact mono text for Akra ownership, the local-context statement, and the source link. It distributes these items across the 1180px frame and wraps below 680px.

## Do's and Don'ts

### Do:

- Do use the actual dashboard screenshot as the main proof of product behavior.
- Do preserve the story order: proposition, mechanism, guided use, portable boundary, download.
- Do keep the GitHub latest-release URL as the working static fallback.
- Do retain Korean semantic structure, visible focus, live release status, and reduced-motion behavior.
- Do keep touch actions at 40px or taller on the narrow layout and prevent horizontal overflow.

### Don't:

- Don't replace the product screenshot with a synthetic canvas, stock device, or generic SaaS mockup.
- Don't invent users, usage figures, benchmarks, compatibility claims, or cloud-service claims.
- Don't let release API failure disable the download path.
- Don't move cyan focus or status meaning into color-only communication.
- Don't reuse the app dashboard's compact instrument typography as the release page's editorial voice.
