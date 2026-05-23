---
name: Architectural Logic
colors:
  surface: '#faf8ff'
  surface-dim: '#d9d9e5'
  surface-bright: '#faf8ff'
  surface-container-lowest: '#ffffff'
  surface-container-low: '#f3f3fe'
  surface-container: '#ededf9'
  surface-container-high: '#e7e7f3'
  surface-container-highest: '#e1e2ed'
  on-surface: '#191b23'
  on-surface-variant: '#434655'
  inverse-surface: '#2e3039'
  inverse-on-surface: '#f0f0fb'
  outline: '#737686'
  outline-variant: '#c3c6d7'
  surface-tint: '#0053db'
  primary: '#003ea8'
  on-primary: '#ffffff'
  primary-container: '#0053db'
  on-primary-container: '#ced8ff'
  inverse-primary: '#b4c5ff'
  secondary: '#495c94'
  on-secondary: '#ffffff'
  secondary-container: '#acbffe'
  on-secondary-container: '#394c83'
  tertiary: '#832600'
  on-tertiary: '#ffffff'
  tertiary-container: '#ac3500'
  on-tertiary-container: '#ffcebf'
  error: '#ba1a1a'
  on-error: '#ffffff'
  error-container: '#ffdad6'
  on-error-container: '#93000a'
  primary-fixed: '#dbe1ff'
  primary-fixed-dim: '#b4c5ff'
  on-primary-fixed: '#00174b'
  on-primary-fixed-variant: '#003ea8'
  secondary-fixed: '#dbe1ff'
  secondary-fixed-dim: '#b4c5ff'
  on-secondary-fixed: '#00174b'
  on-secondary-fixed-variant: '#31447b'
  tertiary-fixed: '#ffdbd0'
  tertiary-fixed-dim: '#ffb59d'
  on-tertiary-fixed: '#390c00'
  on-tertiary-fixed-variant: '#832600'
  background: '#faf8ff'
  on-background: '#191b23'
  surface-variant: '#e1e2ed'
typography:
  headline-lg:
    fontFamily: Inter
    fontSize: 24px
    fontWeight: '600'
    lineHeight: 30px
    letterSpacing: -0.02em
  headline-md:
    fontFamily: Inter
    fontSize: 22px
    fontWeight: '600'
    lineHeight: 28px
    letterSpacing: -0.01em
  headline-sm:
    fontFamily: Inter
    fontSize: 20px
    fontWeight: '600'
    lineHeight: 26px
  title-lg:
    fontFamily: Inter
    fontSize: 16px
    fontWeight: '500'
    lineHeight: 22px
  title-md:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: '500'
    lineHeight: 24px
  body-lg:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: '400'
    lineHeight: 18px
  body-md:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: '400'
    lineHeight: 18px
  label-lg:
    fontFamily: Inter
    fontSize: 14px
    fontWeight: '500'
    lineHeight: 18px
  label-md:
    fontFamily: Inter
    fontSize: 12px
    fontWeight: '500'
    lineHeight: 16px
    letterSpacing: 0.02em
  label-sm:
    fontFamily: Inter
    fontSize: 11px
    fontWeight: '500'
    lineHeight: 16px
    letterSpacing: 0.03em
rounded:
  sm: 0.125rem
  DEFAULT: 0.25rem
  md: 0.375rem
  lg: 0.5rem
  xl: 0.75rem
  full: 9999px
spacing:
  base: 2px
  xs: 2px
  sm: 4px
  md: 8px
  lg: 12px
  xl: 16px
  gutter: 8px
  margin-mobile: 8px
  margin-desktop: 12px
---

## Brand & Style

This design system is built upon the concept of an "Architectural Digital Desktop." It prioritizes structural integrity and spatial logic over decorative elements. The brand personality is authoritative, precise, and highly organized, catering to enterprise environments where information density and clarity are paramount.

The aesthetic leans into **Minimalism** and **Corporate Modernism**, utilizing tonal layering to define workspace boundaries. It avoids the visual noise of traditional borders, instead using subtle shifts in surface values to create a "built" environment. The emotional response is one of reliability, focus, and systematic efficiency.

## Colors

The palette is anchored in "Architectural Grays"—cool-toned neutrals that differentiate functional zones without creating visual friction.

- **Primary Brand Expression:** Used exclusively for high-priority actions and active states. The primary gradient (135° from `#0053db` to `#0048c1`) provides the only significant "pop" of color in the interface.
- **Surface Hierarchy:**
  - `surface_container_low` defines the navigational infrastructure (Sidebars).
  - `surface_container_lowest` defines the primary work surface (Main Content).
  - `surface_container_high` defines interactive secondary elements and utility panels.
- **Typography:** Deep slate tones ensure high legibility while maintaining the cool, professional temperature of the system.

## Typography

The design system utilizes **Inter** exclusively to leverage its exceptional readability in high-density data environments.

The type scale is optimized for enterprise efficiency:

- **Headlines:** Use tighter letter spacing and semi-bold weights to anchor page sections.
- **Body:** Set at a standard 14px (`body-md`) for maximum information density without sacrificing legibility.
- **Labels:** Used for metadata, table headers, and form captions, often utilizing `label-md` or `label-sm` in all-caps for distinct categorization.

## Layout & Spacing

The layout follows a **Fixed-Fluid Hybrid** model. The sidebar navigation is fixed at 256px, while the main content area expands fluently to fill the remaining viewport.

- **Grid:** A 12-column grid is used for the main content area with 8px gutters.
- **Rhythm:** An 4px base unit governs all spatial relationships. High-density views (tables/lists) may drop to a 2px "half-step" to consolidate information.
- **Layering over Borders:** Structure is defined by color blocks. For example, a "Card" is simply a `#ffffff` area sitting on a `#f7f9fb` background. Separation is achieved through value contrast, not lines.

## Elevation & Depth

This design system uses **Tonal Layering** to convey hierarchy.

1. **Level 0 (Floor):** `surface_background` (#f7f9fb) is the canvas.
2. **Level 1 (Infrastructure):** `surface_container_low` (#f0f4f7) for persistent navigation and side-panels.
3. **Level 2 (Worksheet):** `surface_container_lowest` (#ffffff) for primary content cards and data tables.
4. **Level 3 (Interactive):** `surface_container_high` (#e1e9ee) for hovering states, secondary actions, or toolbars.

**Shadows** are strictly reserved for **Floating Elements** (popovers, tooltips, modals). Use a soft, neutral-tinted shadow: `0 4px 16px rgba(42, 52, 57, 0.08)`. Static components remain flat.

## Shapes

The shape language is **Soft** and disciplined. A 0.25rem (2px) base radius is applied to buttons, input fields, and small UI components to maintain a professional edge while feeling modern. Large containers (cards, panels) use 0.5rem (4px) to provide a clear sense of containment within the architectural grid.

## Components

### Buttons

- **Primary:** 135° Linear Gradient (`#0053db` to `#0048c1`), white text. No border.
- **Secondary:** `surface_container_high` background with `on_surface` text. No border. Use for the majority of non-critical actions.
- **Ghost:** No background, `on_surface_variant` text. High-density utility only.

### Inputs

- **Fields:** Filled style using `surface_container_low`.
- **Indicator:** A 2px bottom-accent in `primary` appears only on focus. No 4-sided borders.
- **Text:** `body-md` for input text, `label-md` for persistent labels above the field.

### Cards & Surfaces

- Surfaces should be distinguished by color shifts.
- A "Card" is a white block (`surface_container_lowest`) on a gray background.
- No shadows or borders should be applied to static cards.

### Navigation & Lists

- **Sidebar:** `surface_container_low` background.
- **Active Item:** Indicator bar (4px width) on the left edge in `primary` color, with the item background shifting to `surface_container_high`.
- **Lists:** High-density, 40px row heights for standard tables, 32px for compact views.

### Data Visualization

- Utilize `primary` for main data series.
- Use `on_surface_variant` for axes and grid lines (at 10% opacity).
- Status indicators (dots) use `primary` for active, `error` for critical, and `on_surface_variant` for inactive.
