---
name: Murmur
description: The Focused Voice Workspace — a flat Swiss grid in Office gold, with rounded Control Centre surfaces and Inter throughout.
colors:
  accent: "#d0ad5e"
  on-accent: "#000000"
  accent-ink-dark: "#d0ad5e"
  accent-ink-light: "color-mix(in srgb, #d0ad5e 50%, #0f172a)"
  ring-dark: "#ffffff"
  ring-light: "#0f172a"
  canvas-dark: "#030613"
  surface-dark: "#1a1d29"
  surface-raised-dark: "#252832"
  border-dark: "color-mix(in srgb, #30313a 35%, #9aa0b4)"
  text-dark: "#f8fafc"
  muted-dark: "#94a3b8"
  canvas-light: "#f8fafc"
  surface-light: "#ffffff"
  surface-raised-light: "#e9eef6"
  border-light: "color-mix(in srgb, #475569 72%, #ffffff)"
  text-light: "#0f172a"
  muted-light: "#475569"
  status-ready-dark: "#22c55e"
  status-ready-light: "color-mix(in srgb, #15803d 60%, #0f172a)"
  status-pending-dark: "#f59e0b"
  status-pending-light: "color-mix(in srgb, #a16207 60%, #0f172a)"
  danger-dark: "#f87171"
  danger-light: "#dc2626"
typography:
  family: '"Inter Variable", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
  display:
    fontSize: "clamp(34px, 3.6vw, 52px)"
    fontWeight: 600
    lineHeight: 1.02
    letterSpacing: "-0.04em"
  sectionHeading:
    fontSize: "11px"
    fontWeight: 600
    letterSpacing: "0.12em"
    textTransform: uppercase
  microLabel:
    fontSize: "9px"
    fontWeight: 500
    letterSpacing: "0.16em"
    textTransform: uppercase
  body:
    fontSize: "13.5px"
    fontWeight: 400
    lineHeight: 1.6
  uiText:
    fontSize: "12px"
    fontWeight: 400
    lineHeight: 1.6
  metric:
    fontSize: "20px"
    fontWeight: 600
    fontVariantNumeric: tabular-nums
  keycap:
    fontSize: "10px"
    fontWeight: 500
    letterSpacing: "0.08em"
    textTransform: uppercase
rounded:
  surface: "17px"
  control: "9px"
  keycap: "9px"
  notch: "16px"
  controlCentre: "17px"
  controlCentreShell: "0 0 22px 22px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "20px"
  xxl: "24px"
components:
  nav-link:
    height: "36px"
    typography: "{typography.microLabel}"
    rounded: "{rounded.control}"
    activeIndicator: "inset 2px 0 0 {colors.accent-ink-dark}"
  primary-button:
    backgroundColor: "{colors.accent}"
    textColor: "{colors.on-accent}"
    borderColor: "{colors.accent-ink-dark}"
    height: "36px"
    rounded: "{rounded.control}"
  subtle-button:
    backgroundColor: "transparent"
    textColor: "{colors.muted-dark}"
    height: "30px"
    rounded: "{rounded.control}"
  card:
    backgroundColor: "{colors.surface-dark}"
    borderColor: "{colors.border-dark}"
    padding: "20px"
    rounded: "{rounded.surface}"
  status-chip:
    backgroundColor: "color-mix(in oklab, currentStatus 12%, {colors.surface-dark})"
    borderColor: "color-mix(in oklab, currentStatus 30%, {colors.surface-dark})"
    rounded: "{rounded.control}"
  keycap:
    backgroundColor: "{colors.surface-raised-dark}"
    borderColor: "{colors.border-dark}"
    height: "24px"
    rounded: "{rounded.keycap}"
---

# Design System: Murmur Dashboard

## Overview

**Creative North Star: "The Quiet Instrument"**

The dashboard is a flat Swiss grid borrowed into the Control Centre's material: one visible hairline grid, one gold accent, no shadows, and rounded surfaces instead of ornament. It is a working surface for voice actions, so hierarchy comes from type weight, letter-spacing, rules, and tone — never from elevation.

Three things define the look:

1. **A visible grid.** Content groups are separated by 1px rules, and the page is a table of regions rather than a stack of floating cards.
2. **Control Centre surfaces.** Deep charcoal canvas, slate panels, and gold selection — the same palette as the expanded overlay, so the two windows read as one product.
3. **Micro-label typography.** Almost every label is uppercase, 9–11px, with 0.08–0.16em tracking. Primary values (headline, metric figures) are the only large type.

Radii come in two steps and nothing else: **17px** for surfaces (cards, media, notices) and **9px** for controls (nav rows, buttons, chips, keycaps, icon tiles). A 24px keycap with a 17px radius would read as a pill, and a card with 9px reads as a button — the step, not one number, is what makes the corners look deliberate.

> **Typeface note.** This system was built around Fira Sans + Fira Code. Both were removed from the app; `--font-sans` and `--font-mono` now resolve to **Inter Variable**, matching the Control Centre. Every place that still reads `font-family: var(--font-mono)` is intentionally aliased to Inter: the tracking and uppercase treatment, not a monospaced face, is what defines those labels. Do not reintroduce a second family for the dashboard.

The notch remains a separate overlay language (translucent blur, content-sized shapes). It shares the accent source but not these tokens.

## Typography

**Single family:** `"Inter Variable", "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif`, imported once in `src/main.tsx` via `@fontsource-variable/inter`.

| Role | Size | Weight | Tracking | Case | Used for |
| --- | --- | --- | --- | --- | --- |
| Display | `clamp(34px, 3.6vw, 52px)` | 600 | -0.04em | sentence | Overview `h1`, max 22ch, `text-wrap: balance` |
| Section heading | 11px | 600 | 0.12em | UPPER | Card `h2` |
| Micro label | 9–10px | 500–600 | 0.08–0.16em | UPPER | Nav, breadcrumb, metrics, buttons, status chips |
| Metric figure | 20px | 600 | -0.02em | — | `.dashboard-metric strong`, `tabular-nums` |
| Body | 13.5px / 1.6 | 400 | 0 | sentence | Overview lede |
| UI copy | 11–12.5px / 1.55–1.6 | 400–600 | 0 | sentence | Task rows, notes, empty states |
| Keycap | 10px | 500 | 0.08em | UPPER | `.dashboard-key-combo` / `Keycap` |

### Rules

- **Uppercase is for labels, not sentences.** Never set body copy in caps; never track body copy.
- **Numbers align.** Any figure that can change (counts, durations, metric values) uses `font-variant-numeric: tabular-nums`.
- **Two sizes per block.** A section title plus its content; avoid inventing intermediate display sizes.
- **No second family.** `--font-mono` exists as a token name only, so existing utility classes keep working; it resolves to Inter.

## Colors

Dark is the native mode and takes its values from the Control Centre; light is a role-for-role remap applied by `html[data-theme="light"]`. The user's accent replaces `--color-accent` at runtime, and the default is now Office gold.

| Role | Dark | Light |
| --- | --- | --- |
| Canvas | `#030613` | `#f8fafc` |
| Surface (cards) | `#1a1d29` | `#ffffff` |
| Raised | `#252832` | `#e9eef6` |
| Border | `color-mix(#30313a 35%, #9aa0b4)` → `#757989` | `color-mix(#475569 72%, #ffffff)` → `#7b8593` |
| Text | `#f8fafc` | `#0f172a` |
| Muted text | `#94a3b8` | `#475569` |
| Accent (fill) | `#d0ad5e` | `#d0ad5e` |
| On-accent | `#000000` | `#000000` |
| Accent ink (icons, markers, edges) | `var(--color-accent)` | `color-mix(#d0ad5e 50%, #0f172a)` → `#706244` |
| Focus ring | `#ffffff` | `#0f172a` |
| Status ready | `#22c55e` | `color-mix(#15803d 60%, #0f172a)` → `#135635` |
| Status pending | `#f59e0b` | `color-mix(#a16207 60%, #0f172a)` → `#674415` |
| Danger | `#f87171` | `#dc2626` |

### Named rules

**The Two Golds Rule.** `--color-accent` is a *fill*; `--dashboard-accent-ink` is *ink*. Both are gold in dark, but on white the raw fill is only 1.6:1, so the ink role mixes it halfway to the foreground (`#706244`). Use ink for icons, the active-nav marker, and button edges; use the fill only where text or a border carries the contrast.

**The Ring Rule.** `--dashboard-ring` is white in dark and `#0f172a` in light. Never outline a dashboard control with the accent: gold is 1.6:1 on white and invisible as a ring.

**The Derived Border Rule.** Hairlines must clear 3:1 against the surface. The Control Centre's own `#30313a` border is 1.8:1 on this canvas, so it is mixed toward the muted role to `#757989`. The dashboard scopes the same value to `--color-line` so Tailwind `border-line` utilities lift too; the notch keeps its own border token.

**The Contrast Budget (measured).**

| Pair | Ratio |
| --- | --- |
| Text / surface | 16.0 dark · 17.9 light |
| Muted / surface | 6.5 dark · 7.6 light |
| Status chip text on its 12% tint | 6.0 / 6.4 dark · 7.2 / 7.2 light |
| Hairline on surface | 3.9 dark · 3.7 light |
| Accent ink on surface | 9.5 dark · 6.0 light |
| Black label on accent fill | 9.8 |
| Focus ring on surface | 18.6 dark · 17.9 light |

Changing any role means re-measuring; several earlier values passed visually and failed numerically.

## Layout

Two-column grid: a **224px** sticky sidebar and a fluid main column.

- Sidebar: `padding: 24px 14px 18px`, full height, right hairline, brand block with a bottom rule, nav rows of **36px**.
- Top bar: **48px**, sticky, bottom hairline, horizontal padding `clamp(20px, 3vw, 40px)`.
- Content: `min(100%, 1200px)`, centered, `gap: 20px`, padding `28px clamp(20px, 3vw, 40px) 40px`.
- Overview: `minmax(0,1fr) minmax(240px,300px)`, `gap: 32px`, closed by a rule.
- Work grid: `minmax(0,1.08fr) minmax(0,0.92fr)`, `gap: 20px`, top-aligned.
- Metrics: three equal columns separated by vertical hairlines, first cell flush left.

### Breakpoints

| Width | Behaviour |
| --- | --- |
| ≤ 1060px | Sidebar 200px, overview narrows, work grid collapses to one column, content gap 18px |
| ≤ 760px | Sidebar becomes a static top header, nav becomes horizontally scrollable, overview stacks, content padding `20px 16px 32px`, nav links 34px / 11px |
| ≤ 460px | Heading 32px, card padding 15px, metric padding 9px with 11.5px figures, shortcut rows collapse to one column |

Spacing steps: **4, 8, 12, 16, 20, 24px**. Card padding is 20px; the responsive pass may reduce it.

## Shapes & Elevation

- **Surfaces:** 17px (`--dashboard-radius`) — cards, media frame, notice, empty state.
- **Controls:** 9px (`--dashboard-radius-sm`) — nav rows, buttons, connection chip, status chips, count badge, brand mark, quick-start icon, icon links, keycaps.
- **Dots:** the 6px status mark uses the surface radius, which clamps to a circle.
- **Elevation:** none at rest, none on hover. No `box-shadow` on dashboard surfaces; the raised fill is the only lift.
- **Depth** comes from three tones (canvas → surface → raised), hairlines, and the gold fill.
- **Overlays keep their own shapes:** the notch uses 16px cards and a blur; the Control Centre uses 17px tiles under a `0 0 22px 22px` shell. Share the palette, not the geometry, unless deliberately matching a surface.

## Components

### Navigation

36px rows, uppercase 10px labels at 0.1em, 9px radius, separated by hairlines. Hover/focus fills with `--dashboard-surface-raised` and brightens text. The active item takes the surface fill plus a **2px inset left marker** in accent ink and the same colour on its icon.

### Buttons

| Variant | Height | Padding | Type | Surface |
| --- | --- | --- | --- | --- |
| Primary | 36px | 0 12px | 10px / 600 / 0.1em UPPER | Gold fill, black label, accent-ink border |
| Subtle / Preview | 30px | 0 10px | same | Transparent, muted text, border on hover |
| Text link | inline | — | 10px / 0.1em UPPER | None; brightens to text colour |

Primary hover mixes the accent 86% with the dashboard text. Pressed states drop to `opacity: 0.85`; disabled to `0.55` with `not-allowed`. The primary button's border exists so the gold fill has a 3:1+ edge on light backgrounds.

### Cards

Surface fill, 1px hairline, 20px padding, 17px radius, no shadow. Section headings are 11px uppercase at 0.12em with a 12px muted description beneath.

### Keys & keycaps

`Keycap` renders a 24px-tall `kbd`: 1px border, `--color-panel-raised` fill, 9px radius, 10px uppercase text at 0.08em. Combinations join caps with a muted `+`. The 104px first column of a shortcut row reserves room for the combo.

### Status & state

- **Status chips:** uppercase 10px, 9px radius, 1px border and 12% tint derived from the status colour via `color-mix(… , var(--dashboard-surface))`. The status colour must be legible *on its own tint*; that is why light-mode status tokens are darkened.
- **Connection chip:** 26px, hairline border, 9px radius, dot + uppercase label; colour switches between ready, pending, and muted.
- **Count badge:** 26×24px, hairline border, 9px radius, tabular figures.
- **Notice:** 17px surface, hairline, 12px copy, 24px dismiss target.
- **Empty state:** dashed hairline, 17px radius, muted 11.5px copy.

### Media frame

A 17px frame with a hairline, holding a grayscale image (`filter: grayscale(1) contrast(1.05)`) at 0.72 opacity under a 30% charcoal shade (light: 0.85 opacity, 32% light shade). It is the one place a photograph appears; it stays desaturated so it cannot compete with the gold.

## Motion

- Transitions are **200ms ease** on `color`, `background-color`, `border-color`, and `box-shadow` only.
- Hover changes tone; it does not move, scale, or lift.
- Pressed feedback is opacity, not transform.
- `@media (prefers-reduced-motion: reduce)` zeroes animation and transition durations inside `.dashboard-shell` and removes nav/button transitions entirely.

## Accessibility

- **Focus:** `html[data-window="main"] .dashboard-shell :focus-visible` draws a 2px `--dashboard-ring` outline at 3px offset; nav and connection links use the same ring at 2px offset. Never remove the outline.
- **Colour is never the only cue:** status chips carry text labels, the active nav item has a marker plus a fill, and the connection chip shows a dot plus a word.
- **Live updates:** the task count is announced through a polite `sr-only` live region; decorative status dots are `aria-hidden`.
- **Targets:** interactive rows are ≥ 24px tall; primary actions are 36px; the notice dismiss is 24px.
- **Contrast:** token pairs are budgeted above (4.5:1 text, 3:1 non-text) and were measured in both themes.

## Theme switching

`applyTheme()` in `src/lib/theme.ts` sets `data-theme`, `color-scheme`, and the runtime accent variables on `<html>`:

- `--color-accent` — the user's accent; the default is `#d0ad5e`. Rust's `DEFAULT_ACCENT` and the Loadout's swatch list carry the same gold.
- `--color-on-accent` — black or white, chosen from the accent's relative luminance (> 0.179 → black); gold resolves to black.
- `--color-accent-soft` — set inside `paint()`, so it follows `system` mode changes: pale mix on dark, foreground mix on light.
- `--notch-accent` — the overlay accent, separate from the dashboard.

Everything dashboard-specific is a `--dashboard-*` token declared on `.dashboard-shell` and overridden in the light block. If you add a role, add both modes in the same change.

## Do / Don't

**Do**
- Use the 17px surface radius and the 9px control radius; nothing else.
- Keep surfaces flat and separated by hairlines.
- Use the uppercase micro-label for anything that names a field, action, or state.
- Derive new colours from the palette roles (mix toward text/muted/foreground) instead of introducing new hexes.
- Keep `tabular-nums` on any changing figure.
- Re-measure contrast when a token changes; add both theme values together.

**Don't**
- Don't add gradients, glows, or resting shadows to dashboard elements.
- Don't put 17px on a 24–36px control, or 9px on a card; the radius step is part of the hierarchy.
- Don't use `--color-accent` as ink, a focus ring, or a hairline; use `--dashboard-accent-ink`, `--dashboard-ring`, and `--dashboard-border`.
- Don't reintroduce Fira, a serif, or a second display face.
- Don't assume a visually fine colour passes: the Control Centre border and the raw gold both failed 3:1/4.5:1 on light before being mixed.
