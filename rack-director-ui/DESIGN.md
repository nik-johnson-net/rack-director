# Rack Director UI Design Specification

This document is the authoritative design reference for all UI components and pages.
Agents building UI components MUST follow this spec exactly.

## Design Preview

See `design-preview.html` for the interactive prototype. Open it in a browser to see all pages.

## Design Philosophy

- **Dark terminal aesthetic**: Monospace font, dark backgrounds, minimal color
- **Dense and functional**: Every pixel is information, no decorative elements
- **Color for status only**: Background surfaces are monochrome; color indicates state
- **Sharp geometry**: 0-4px border radius, 1px borders
- **Homelab operator UX**: Quick access to common workflows, progressive disclosure for details

## Technology

- React 19 + TypeScript
- Tailwind CSS 4 (using `@theme` syntax)
- shadcn/ui (Radix primitives)
- Lucide React icons
- TanStack React Table v8
- JetBrains Mono font (via Google Fonts or self-hosted)

## Design Tokens

### Colors

```css
/* Backgrounds */
--bg-base: #0d1117;        /* Page background */
--bg-surface: #161b22;     /* Cards, sidebar, table rows (odd) */
--bg-raised: #1c2128;      /* Table headers, active nav, hover states */
--bg-overlay: #21262d;     /* Dropdowns, tooltips, modals */

/* Borders */
--border: #30363d;         /* Primary borders */
--border-muted: #21262d;   /* Subtle borders (between table rows) */

/* Text */
--text-primary: #e6edf3;   /* Headings, data values, primary content */
--text-secondary: #8b949e; /* Labels, column headers, descriptions, inactive nav */
--text-muted: #484f58;     /* Placeholders, disabled text, hints */

/* Accent */
--accent: #00d4aa;         /* Primary CTA, active indicators, progress bars, links */
--accent-hover: #00f0c0;   /* Hover state for accent elements */
--accent-muted: rgba(0, 212, 170, 0.15); /* Accent backgrounds */

/* Status Colors (foreground) */
--status-new: #4a9eed;
--status-unprovisioned: #f59e0b;
--status-provisioned: #22c55e;
--status-broken: #ef4444;
--status-removed: #8b949e;
--status-transitioning: #06b6d4;

/* Status Colors (background - 12% opacity of foreground) */
--status-new-bg: rgba(74, 158, 237, 0.12);
--status-unprovisioned-bg: rgba(245, 158, 11, 0.12);
--status-provisioned-bg: rgba(34, 197, 94, 0.12);
--status-broken-bg: rgba(239, 68, 68, 0.12);
--status-removed-bg: rgba(139, 148, 158, 0.12);
--status-transitioning-bg: rgba(6, 182, 212, 0.12);

/* Semantic */
--warn-bg: rgba(245, 158, 11, 0.08);
--warn-border: rgba(245, 158, 11, 0.3);
--error-bg: rgba(239, 68, 68, 0.08);
--error-border: rgba(239, 68, 68, 0.3);
```

### Typography

```
Font Family: 'JetBrains Mono', 'Cascadia Code', 'Fira Code', monospace
  - Used EVERYWHERE. No sans-serif. This is the terminal aesthetic.

Font Sizes:
  --text-xs:  11px  (badges, tiny labels)
  --text-sm:  12px  (table cells, form inputs, nav items, secondary text)
  --text-base: 13px (body text - rarely used, most text is sm)
  --text-lg:  15px  (section titles)
  --text-xl:  18px  (page subtitles)
  --text-2xl: 22px  (page titles)

Font Weights:
  400 (normal) - body text, table cells
  500 (medium) - buttons, badges
  600 (semibold) - headings, labels, card titles
  700 (bold) - logo, stat card values
```

### Spacing

```
--space-1: 4px
--space-2: 8px
--space-3: 12px
--space-4: 16px
--space-5: 20px
--space-6: 24px
--space-8: 32px
```

### Border Radius

```
--radius-sm: 2px   (badges, small elements)
--radius:    4px   (buttons, inputs, cards)
--radius-lg: 6px   (modals, larger containers)
```

## Component Specifications

### Sidebar
- Width: 200px fixed
- Background: var(--bg-surface)
- Right border: 1px solid var(--border)
- Logo: "RACK" in accent color, 18px bold. "director" below in secondary, 11px uppercase, letter-spacing 3px
- Separator: 1px solid var(--border), horizontal, with 20px horizontal padding
- Nav items: 12px font, secondary color, 8px vertical padding, 20px left padding
- Active nav item: primary text color, bg-raised background, 3px accent left border
- Hover: primary text color, bg-raised background
- Icons: Lucide icons, 16px, before each nav label
- Bottom section: Settings link, separated by border-top

### Page Header
- Title: 22px, semibold, primary color
- Subtitle: 12px, secondary color, 4px below title
- Action buttons: flex row, right-aligned, same vertical level as title
- Breadcrumbs: above title, 11px, muted color, "/" separators, links in secondary color

### Stat Card
- Background: var(--bg-surface)
- Border: 1px solid var(--border)
- Padding: 16px
- No border-radius (sharp corners)
- Label: 11px, uppercase, letter-spacing 1px, status color matching the lifecycle state
- Value: 28px, bold, primary color
- Detail: 11px, secondary color
- Hover: border-color lightens, background becomes bg-raised
- Clickable: navigates to filtered device list

### Button
- Font: JetBrains Mono, 12px, medium weight
- Padding: 8px 16px
- Border-radius: 4px
- **Primary**: bg accent, text bg-base (dark on bright), border accent. Hover: accent-hover
- **Secondary/Default**: bg-surface, text primary, border. Hover: bg-raised, border lightens
- **Danger**: bg-surface, text broken-red, border error-border. Hover: error-bg
- **Ghost**: transparent bg, text secondary. Hover: bg-raised
- Keyboard shortcut hint: small badge inside button (rgba white bg, xs text, secondary color)

### Table
- Wrapped in border container (1px solid var(--border))
- Header row: bg-raised, 11px uppercase, letter-spacing 0.5px, secondary color, semibold
- Header padding: 8px 12px
- Body rows: alternating bg-surface / bg-base
- Body padding: 8px 12px, 12px font
- Row hover: bg-raised
- Row borders: 1px solid border-muted between rows
- Last row: no bottom border
- Links in table: accent color, no underline

### Status Badge
- Inline-flex, centered
- Padding: 2px 8px
- Border-radius: 2px
- Font: 11px, medium weight
- Colored dot (6px circle) before text
- Background: status-*-bg (12% opacity)
- Text: status-* color
- Transitioning badge dot: pulse animation (opacity 1 → 0.3, 2s infinite)

### Form Elements
- **Label**: 11px, semibold, secondary color, uppercase, letter-spacing 0.5px, 4px margin-bottom
- **Input**: bg-base, border, 12px font, 8px 12px padding, 2px radius
- **Input focus**: accent border, 1px accent box-shadow
- **Input error**: broken-red border
- **Error message**: 11px, broken-red, 4px margin-top
- **Hint text**: 11px, muted color, 4px margin-top
- **Select**: same as input, custom chevron arrow (svg), 28px right padding
- **Form group**: 16px margin-bottom

### Card
- Background: var(--bg-surface)
- Border: 1px solid var(--border)
- Padding: 16px
- No border-radius
- Title: 13px, semibold, primary color, 12px margin-bottom
- Can contain KV grids, tables, or form groups

### KV Grid (Key-Value display)
- CSS Grid: 140px label column, 1fr value column
- Key: 11px, secondary, uppercase, letter-spacing 0.5px
- Value: 12px, primary
- Row gap: 4px, column gap: 16px

### Warning Row
- Flex row, 8px 12px padding
- 3px left border (colored by severity)
- Error: error-bg background, broken-red left border
- Warning: warn-bg background, unprovisioned-amber left border
- Device name: primary color, 500 weight, 90px min-width
- Message: secondary color
- Dismiss button: right-aligned, muted color, hover secondary

### Progress Bar
- Container: 80px wide, 6px tall, bg-overlay, 3px radius
- Fill: accent color, 3px radius
- Percentage text: 11px, secondary, 8px left margin

### Page Tabs
- Flex row, no gap
- Bottom border: 1px solid var(--border)
- Tab: 8px 16px padding, 12px font, secondary color
- Active tab: primary color, 2px accent bottom border
- Hover: primary color
- Margin-bottom: 24px below tabs

### Breadcrumbs
- 11px font, muted color
- Links: secondary color, hover accent
- Separator: "/" character, muted color
- 12px margin-bottom

### Empty State
- Centered text, 32px top/bottom padding
- Icon: 32px, 50% opacity
- Title: 15px, primary color
- Description: 12px, secondary color

### Section Header
- Flex row, space-between
- Title: 15px, semibold, primary
- Link: 11px, accent color, hover accent-hover
- 12px margin-bottom

## Page Specifications

### Dashboard (/)
1. Page header: "Dashboard" / "Fleet overview and quick actions"
2. Stat grid: 4 cards (New, Unprovisioned, Provisioned, Broken) - auto-fit grid, min 150px
3. Quick Actions section: row of buttons (Provision Device [primary], Deprovision Device, Create Role, Add OS Image)
4. Active Transitions section: table with Device, Action, Status, Progress columns
5. Warnings section: warning rows with dismiss buttons

### Devices (/devices)
1. Breadcrumbs + Page header with "+ Add Pending Device" button
2. Filter bar: search input (260px), lifecycle select, platform select, role select
3. Table: Hostname, MAC, Platform, Role, Lifecycle (badge), Actions (view link)

### Device Detail (/devices/:uuid)
1. Breadcrumbs + Page header with device name + status badge, action buttons right-aligned
2. Page tabs: Overview, Hardware, Transitions, Warnings
3. Overview tab: 2-column grid of cards (Identity KV, Network Interfaces table, Disks table, Recent Transitions table)

### Roles (/roles)
1. Table: Name, OS, Firmware, Disk Layout summary, Device count, edit link

### Role Edit (/roles/:id)
1. General card: name input, OS select, firmware mode select (2-column grid)
2. Disk Layout card: disk device blocks with partition tables
   - Each disk block: colored header (label in accent, partition type badge), partition table, "+ Add Partition" link
   - Partition row: mount, size, filesystem, flags (badges), delete button

### Platforms (/platforms)
1. Table: Name, Firmware, Disks summary, NICs, Memory, Device count, view link

### OS Images (/operating-systems)
1. Table: Name, Version, Architectures (badges), Used By roles, edit link

### Networks (/networks)
1. Table: Name, Subnet, Gateway, DNS, Autodiscovery (badge), Lease count, view link

### Settings (/settings)
1. Simple card-based layout for any configuration options
