---
name: Murmur
description: The Focused Voice Workspace — a precise Windows voice-assistant interface built to recede into active work.
colors:
  accent: "#7c8cff"
  accent-soft: "#a7b2ff"
  accent-cyan: "#22d3ee"
  accent-violet: "#a855f7"
  accent-orange: "#f97316"
  accent-green: "#10b981"
  accent-rose: "#f43f5e"
  canvas-dark: "#08090d"
  panel-dark: "#0e1016"
  panel-raised-dark: "#13151d"
  line-dark: "#1f2230"
  muted-dark: "#a1a6b5"
  text-dark: "#e7e9f2"
  canvas-light: "#eef0f5"
  panel-light: "#ffffff"
  panel-raised-light: "#f6f7fb"
  line-light: "#dde1eb"
  muted-light: "#535a6b"
  text-light: "#171a23"
  dashboard-surface-dark: "rgba(17, 20, 27, 0.96)"
  dashboard-raised-dark: "rgba(23, 27, 35, 0.98)"
  dashboard-border-dark: "rgba(255, 255, 255, 0.1)"
  dashboard-text-dark: "#edf1f8"
  dashboard-muted-dark: "#a9afc1"
  dashboard-surface-light: "#ffffff"
  dashboard-raised-light: "#f3f5f9"
  dashboard-border-light: "rgba(22, 27, 40, 0.12)"
  dashboard-text-light: "#202431"
  dashboard-muted-light: "#5b6274"
  notch-dark-surface: "rgba(11, 12, 17, 0.94)"
  notch-light-surface: "rgba(255, 255, 255, 0.94)"
  notch-dark-accent: "#a7b2ff"
  notch-light-accent: "#5b63d3"
typography:
  display:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "clamp(34px, 3.4vw, 46px)"
    fontWeight: 580
    lineHeight: 1.08
    letterSpacing: "-0.055em"
  title:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "15px"
    fontWeight: 620
    lineHeight: 1.5
  body:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.5
  button:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "12px"
    fontWeight: 550
    lineHeight: 1.5
  navigation:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "12px"
    fontWeight: 500
    lineHeight: 1.5
  input:
    fontFamily: '"Cascadia Code", ui-monospace, SFMono-Regular, monospace'
    fontSize: "14px"
    fontWeight: 400
    lineHeight: 1.43
  chip:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "11px"
    fontWeight: 500
    lineHeight: 1.5
  label:
    fontFamily: '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif'
    fontSize: "10px"
    fontWeight: 650
    lineHeight: 1.4
    letterSpacing: "0.13em"
rounded:
  chip: "8px"
  control: "10px"
  navigation: "12px"
  surface: "16px"
  notch-answer: "20px"
  pill: "999px"
spacing:
  xs: "4px"
  sm: "8px"
  md: "12px"
  lg: "16px"
  xl: "24px"
components:
  button-primary:
    backgroundColor: "{colors.accent-soft}"
    textColor: "#10121a"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "0 14px"
    height: "40px"
  button-subtle:
    backgroundColor: "{colors.dashboard-raised-dark}"
    textColor: "{colors.dashboard-muted-dark}"
    typography: "{typography.button}"
    rounded: "{rounded.control}"
    padding: "0 11px"
    height: "34px"
  input-key:
    backgroundColor: "{colors.canvas-dark}"
    textColor: "#e4e4e7"
    typography: "{typography.input}"
    rounded: "{rounded.chip}"
    padding: "0 12px"
    height: "44px"
  navigation-active:
    backgroundColor: "color-mix(in oklab, {colors.accent} 10%, {colors.dashboard-surface-dark})"
    textColor: "{colors.dashboard-text-dark}"
    typography: "{typography.navigation}"
    rounded: "{rounded.navigation}"
    padding: "0 10px"
    height: "42px"
  card-surface:
    backgroundColor: "{colors.dashboard-surface-dark}"
    textColor: "{colors.dashboard-text-dark}"
    rounded: "{rounded.surface}"
    padding: "22px"
  chip-selected:
    backgroundColor: "color-mix(in oklab, {colors.accent} 15%, transparent)"
    textColor: "{colors.accent-soft}"
    typography: "{typography.chip}"
    rounded: "{rounded.chip}"
    padding: "6px 10px"
  notch-answer:
    backgroundColor: "{colors.notch-dark-surface}"
    textColor: "#f2f3f9"
    rounded: "{rounded.notch-answer}"
  control-toggle:
    backgroundColor: "rgba(255, 255, 255, 0.055)"
    textColor: "#f3f5ff"
    rounded: "11px"
    padding: "8px 10px"
    height: "58px"
---

# Design System: Murmur

## Overview

**Creative North Star: "The Focused Voice Workspace"**

Murmur is a restrained desktop workspace for voice-driven actions, not a promotional surface. The interface feels precise, dependable, and low-distraction: graphite-like tonal layers, compact labels, and a muted periwinkle cue make state changes legible without competing with the task at hand.

The main dashboard uses both dark and light palettes and keeps its surfaces mostly flat. A separate transient language belongs to the notch and Control Centre: translucent, blurred, and elevated because these panels float over other work. That distinction preserves the in-app workspace as quiet while giving overlays enough separation to be read at a glance.

**Key Characteristics:**
- Graphite neutrals with a configurable periwinkle-first accent.
- Compact, clear hierarchy in Segoe UI Variable Text.
- Tonal surfaces and fine borders do most of the dashboard layering.
- Glass, blur, and stronger shadows are scoped to floating surfaces.

## Colors

The default palette is muted periwinkle over graphite; the Loadout allows the user to select alternate accents, while dark and light modes remap the neutral surfaces and text.

### Primary
- **Muted Periwinkle** (`colors.accent`): Default accent for selected controls, focus cues, and the Loadout accent setting.
- **Periwinkle Tint** (`colors.accent-soft`): Softer foreground and filled action color used with the default accent.
- **User Accent Options** (`colors.accent-cyan`, `colors.accent-violet`, `colors.accent-orange`, `colors.accent-green`, `colors.accent-rose`): Actual selectable Loadout alternatives; they replace the active accent rather than forming a second simultaneous palette.

### Neutral
- **Graphite Canvas, Panel, Raised Panel, Divider, Muted Text, and Primary Text** (`colors.canvas-dark`, `colors.panel-dark`, `colors.panel-raised-dark`, `colors.line-dark`, `colors.muted-dark`, `colors.text-dark`): Dark-mode foundation for the app and its controls.
- **Pale Canvas, White Panel, Raised Panel, Divider, Muted Text, and Primary Text** (`colors.canvas-light`, `colors.panel-light`, `colors.panel-raised-light`, `colors.line-light`, `colors.muted-light`, `colors.text-light`): Light-mode remapping, applied by the theme setting.
- **Dashboard Surface, Raised Surface, Border, Text, and Muted Text** (`colors.dashboard-surface-dark`, `colors.dashboard-raised-dark`, `colors.dashboard-border-dark`, `colors.dashboard-text-dark`, `colors.dashboard-muted-dark`): Main-window tonal surfaces and content colors; the light variants use the matching `*-light` tokens.
- **Notch Surface and Accent** (`colors.notch-dark-surface`, `colors.notch-light-surface`, `colors.notch-dark-accent`, `colors.notch-light-accent`): Separate overlay treatment; the active accent can still be supplied by the user's Loadout.

### Named Rules
**The Theme Pair Rule.** Keep dark and light mappings together: changing a neutral role in one mode must preserve the same hierarchy in the other.

## Typography

**Display Font:** Segoe UI Variable Text (with Segoe UI, system-ui, sans-serif fallbacks)  
**Body Font:** Segoe UI Variable Text (with Segoe UI, system-ui, sans-serif fallbacks)  
**Label/Mono Font:** Segoe UI Variable Text for interface labels; Cascadia Code with ui-monospace fallbacks for code and key-like data.

**Character:** Compact and practical, with a restrained variable-weight range. The overview headline carries the largest scale; most dashboard copy stays in small, legible utility sizes.

### Hierarchy
- **Display** (weight 580, responsive 34–46px, line-height 1.08, letter-spacing -0.055em): The dashboard's main heading.
- **Title** (weight 620, 15px, line-height 1.5): Section headings and card titles.
- **Body** (weight 400, 16px, line-height 1.5): Base document text; component descriptions commonly step down to 12–14px.
- **Button** (weight 550, 12px, line-height 1.5): Main dashboard actions.
- **Label** (weight 650, 10px, letter-spacing 0.13em): Compact section labels. This describes existing utility labels, not a requirement to add eyebrow copy.

### Named Rules
**The Task-First Type Rule.** Reserve the largest size for the current section's primary heading; keep secondary information quieter and avoid widening the hierarchy with display styles.

## Layout

The main window is a two-column dashboard with a 216px sticky sidebar and a centered content region capped at 1160px. At 1060px the sidebar narrows to 192px and the work grid becomes one column. At 760px the sidebar becomes a top header with horizontally scrollable navigation, the overview stacks, and content padding tightens. At 460px the heading reduces to 32px, cards use 15px padding, and shortcut rows stack.

The Control Centre is a separate compact surface: below 700px its header compresses, the body scrolls, content grids collapse to one column, and app, wallpaper, and power choices use two columns. Notch states size to their content; listening and thinking remain compact, while answers expand into a bounded card. Reused spacing is visible in 4, 8, 12, 16, and 24px steps, with larger section gaps set locally.

## Elevation & Depth

The dashboard is flat by default: tonal fills, hairline borders, and active-state color establish hierarchy without resting shadows. Elevation is reserved for windows that float above the desktop. The notch uses a deep shadow and blur; the Control Centre combines a translucent surface, inset highlight, blur, and an outer shadow. The Loadout's selected center mark also has a localized accent glow to identify the active selection.

### Shadow Vocabulary
- **Notch overlay, dark** (`0 24px 70px -24px rgba(0, 0, 0, 0.9)`): The floating dark notch surface.
- **Notch overlay, light** (`0 20px 55px -24px rgba(15, 18, 30, 0.34)`): The floating light notch surface.
- **Control Centre glass** (`inset 0 1px 0 rgba(255, 255, 255, 0.17), 0 18px 48px rgba(0, 0, 0, 0.38)`): Separation for the expanded floating panel.
- **Loadout selection glow** (`0 0 30px -8px color-mix(in oklab, var(--color-accent) 60%, transparent)`): The selected center mark only.

### Named Rules
**The Flat-First Rule.** Do not add resting elevation to ordinary dashboard cards; tonal layers and borders already define their edges.

**The Overlay Glass Rule.** Keep blur and stronger elevation on the notch and Control Centre, where translucency separates floating windows from the desktop.

## Shapes

The dashboard favors gently rounded rectangles: 8px chips and fields, 10px primary controls, 12px navigation items, and 16px cards. Pills use a full radius. The notch changes silhouette by state: a squared lower edge at idle, a capsule while listening or thinking, and a softly rounded expanded answer. Borders are thin and low-contrast in the dashboard, more visible on the floating Control Centre.

## Components

### Buttons
Dashboard actions are restrained, with clear interactive states and practical targets.
- **Shape:** Gently rounded control corners (10px).
- **Primary:** Soft accent fill, dark ink, and a 40px minimum height with 14px horizontal padding.
- **Hover / Focus:** The fill lifts slightly toward white on hover; focus-visible uses a 2px accent outline with a 3px offset. Pressed buttons scale to 0.98.
- **Secondary / Ghost:** Secondary actions use a raised tonal fill and muted text (34px minimum height); text links remain unboxed until interaction.

### Chips
- **Style:** Loadout choices use a thin border and small rounded corners (8px); selected choices receive an accent-tinted fill and softer accent text.
- **State:** Selected and unselected options remain distinguishable by both border and fill; status chips use semantic colors for pending, running, completed, and error states.

### Cards / Containers
- **Corner Style:** Soft 16px corners.
- **Background:** Dashboard cards use the dashboard surface tokens for the current theme.
- **Shadow Strategy:** Flat at rest; see Elevation & Depth for the separate overlay language.
- **Border:** A thin, low-contrast outline separates card from canvas.
- **Internal Padding:** 22px on standard dashboard surfaces; at narrow widths the stylesheet reduces this to 15px.

### Inputs / Fields
- **Style:** Dark canvas fill, line border, 8px corners, and 44px key-field height.
- **Focus:** Accent border plus a subtle 2px accent ring on the API-key field; task-edit inputs use an accent border shift.
- **Error / Disabled:** Validation errors use semantic red text and a restrained tinted border/background; disabled actions reduce opacity.

### Navigation
- **Style:** The desktop sidebar uses 42px rows, 12px corners, and compact 12px labels. Active state is a low-opacity accent surface with brighter text and icon.
- **Hover / Focus:** A faint text-tinted surface appears on hover; keyboard focus is a 2px accent outline.
- **Mobile:** At 760px and below, the sidebar becomes a horizontal, scrollable top navigation.

### Notch
The notch is a separate, content-sized floating control. Idle, listening, and thinking stay compact; answer and error states expand with their own radii and semantic treatment. It follows light/dark overlay colors independently from the dashboard's opaque surfaces, uses a translucent blurred backdrop, and honors reduced motion.

### Control Centre
The expanded Control Centre is a dedicated glass panel with a 23px outer radius, 16px internal cards, compact 9–12px text, active accent tabs, and restrained hover/pressed states. At reduced-transparency preference it removes blur and background imagery rather than changing the dashboard's surface language.

## Do's and Don'ts

### Do:
- **Do** use the actual theme tokens for the selected accent and paired dark/light neutrals.
- **Do** keep dashboard hierarchy primarily tonal, with visible focus and selected states.
- **Do** preserve the notch and Control Centre as distinct, floating overlay surfaces.
- **Do** let responsive dashboard navigation become horizontally scrollable instead of squeezing its labels.

### Don't:
- **Don't** add decorative effects that compete with the user's active task.
- **Don't** spread backdrop blur or heavy shadows from the overlays into ordinary dashboard cards.
- **Don't** treat the existing Overview eyebrow or its large preview composition as a reusable system rule; those remain surface-specific implementation details.
