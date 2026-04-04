# OVERVIEW

- The web UI is a vite + React project. Build by running `npx vite build` in the rack-director-ui/ directory.
- Individual pages exist under rack-director-ui/src/pages/ and global components are under rack-director-ui/src/components/.
- The Web UI depends on the rack-director service running. The project Makefile has a command to do this. Run `make devserver` to expose the web service on 127.0.0.1:3000.
- After making changes, claude should run the web server and visit the page to ensure the page works correctly.
- Web UI must only depend on /ui/ endpoints in rack-director! Never /api/ or /cnc/!

# UI DEVELOPMENT

## Design Spec
**IMPORTANT**: All UI changes must follow `DESIGN.md` — the authoritative design reference for all components and pages.

Before making ANY UI changes, read `DESIGN.md`. It covers:
- Design tokens (colors, typography, spacing, radii)
- Component specifications (sidebar, buttons, tables, cards, forms, badges, etc.)
- Page layouts for every route

All UI changes should use the **ui-developer** subagent.

## Technology Stack
- **Framework**: React 19 + TypeScript + Vite
- **Styling**: Tailwind CSS 4.1, dark-only theme, design tokens in `src/index.css` `@theme` block
- **Font**: JetBrains Mono (monospace everywhere — terminal aesthetic)
- **Components**: Radix UI primitives (via shadcn/ui), restyled to match design
- **Router**: React Router 7.8.2
- **Icons**: Lucide React
- **Tables**: Plain HTML tables (not TanStack Table)

## Design Language
- **Dark terminal aesthetic**: `#0d1117` background, `#00d4aa` cyan-green accent
- **Dense and functional**: 13px base font, compact spacing, no decorative elements
- **Color for status only**: Monochrome surfaces, color indicates lifecycle state
- **Sharp geometry**: No border-radius on cards, 4px on buttons, 2px on badges
- **Monospace everywhere**: JetBrains Mono for all text

## Theme Colors (Tailwind classes)
```
Backgrounds: bg-bg-base, bg-bg-surface, bg-bg-raised, bg-bg-overlay
Borders:     border-border, border-border-muted
Text:        text-text-primary, text-text-secondary, text-text-muted
Accent:      text-accent, bg-accent, border-accent, text-accent-hover
Status:      text-status-new, text-status-unprovisioned, text-status-provisioned,
             text-status-broken, text-status-removed, text-status-transitioning
             (each has a bg-status-*-bg variant for tinted backgrounds)
```

Never use hardcoded colors. Always use the token classes above.

## Key Component Patterns

### PageHeader Component
All pages use `PageHeader` with breadcrumbs, title, subtitle, and action buttons.

### Tables
Simple HTML `<table>` elements with:
- Border wrapper: `border border-border`
- Header: `bg-bg-raised`, 11px uppercase, secondary color
- Rows: alternating `bg-bg-surface` / `bg-bg-base`, hover `bg-bg-raised`

### Cards
`bg-bg-surface border border-border p-4` — no border-radius (sharp corners).

### Forms
- Labels: 11px, uppercase, secondary color, letter-spacing
- Inputs: `bg-bg-base border-border`, accent focus ring
- Selects: same as inputs with custom SVG chevron

## Build Verification
Always run `npm run build` after making UI changes to verify they compile correctly.

## Modals and Dialogs

- Modals should be separate component files in the relevant `components/` subdirectory
- Never embed complex dialog logic directly in page components
- Use the generic `DeleteConfirmationDialog` for delete confirmations
- Modal components should accept `open` and `onOpenChange` props (controlled pattern)
- Modal components should handle their own loading state
- Keep modal logic self-contained - pass callbacks for actions
- Reset form state when modal opens using `useEffect` with `open` dependency

Available modal components:
- `DeleteConfirmationDialog` (`components/ui/delete-confirmation-dialog.tsx`) - Generic delete confirmation
- `MakeStaticDialog` (`components/networks/make-static-dialog.tsx`) - Convert DHCP lease to static reservation
- `PoolDialog` (`components/networks/pool-dialog.tsx`) - Create/edit DHCP pools
- `TransitionDialog` (`components/devices/transition-dialog.tsx`) - Device lifecycle transitions

## Component Extraction

Complex cards and sections should be extracted into separate components when they:
- Have their own state management (selection, loading states)
- Perform API calls or complex logic
- Are more than ~50 lines of JSX
- Could be reused across pages

Available extracted components:
- `PlatformAssignment` (`components/devices/platform-assignment.tsx`) - Platform selection and assignment
- `BmcConfiguration` (`components/devices/BmcConfiguration.tsx`) - BMC network configuration
- `EditableHostname` (`components/devices/editable-hostname.tsx`) - Inline hostname editing

# VALIDATION

rack-director-ui uses a reusable validation framework for all forms.

## Quick Start

See `@.claude/docs/validation-guide.md` for complete documentation.

### Adding Validation to a Form

```typescript
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { FormFieldError } from "@/components/ui/form-field-error";
import { ValidationError } from "@/lib/client";

const { clearAllErrors, clearFieldError, setErrors, hasError, getError } = useFieldErrors();

// In submit handler:
const handleSubmit = async (e: React.FormEvent) => {
  e.preventDefault();
  clearAllErrors();

  try {
    await createFoo({ ...formData });
    navigate("/success");
  } catch (err) {
    if (err instanceof ValidationError) {
      setErrors(err.errors);
      setError("Please fix the validation errors");
    } else {
      setError("Failed to create foo");
    }
  }
};

// In form:
<Input
  onChange={(e) => {
    setValue(e.target.value);
    clearFieldError("field_name");
  }}
  aria-invalid={hasError("field_name")}
/>
<FormFieldError error={getError("field_name")} />
```

## Key Files
- `src/lib/client.ts` - ValidationError class + handleApiError()
- `src/hooks/useFieldErrors.ts` - useFieldErrors hook
- `src/components/ui/form-field-error.tsx` - FormFieldError component

## Important Notes
- Field names in frontend must match backend validation keys (usually snake_case)
- Clear errors when field values change using `clearFieldError()`
- Use `aria-invalid` attribute for accessibility
- All API client functions should use `handleApiError()` for consistent error handling
