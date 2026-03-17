# OVERVIEW

- The web UI is a vite + React project. Build by running `npx vite build` in the rack-director-ui/ directory.
- Individual pages exist under rack-director-ui/src/pages/ and global components are under rack-director-ui/src/components/.
- The Web UI depends on the rack-director service running. The project Makefile has a command to do this. Run `make devserver` to expose the web service on 127.0.0.1:3000.
- The Web UI uses shadcn components for common UI components. Use the internet to search for relevant components that we don't already have at `ui.shadcn.com`.
- After making changes, claude should run the web server and visit the page to ensure the page works correctly.
- Web UI must only depend on /ui/ endpoints in rack-director! Never /api/ or /cnc/!

# UI DEVELOPMENT

## Design Guide
**IMPORTANT**: All UI changes must use the ui-developer subagent.

The design guide covers:
- Core design principles (Consistency, Clarity, Efficiency, Responsiveness)
- Component patterns (Page structure, Forms, Tables, Navigation)
- Theme & styling guidelines (Colors, spacing, typography)
- Accessibility requirements (WCAG AA compliance)
- Testing requirements
- Common patterns and anti-patterns

**Before making ANY UI changes, read the design guide.**

## UI Developer Agent
For UI-focused tasks, consider using the specialized UI Developer agent defined in `DESIGN_GUIDE.md`.

The UI Developer agent is trained to:
- Follow design guide patterns automatically
- Ensure visual consistency across pages
- Apply responsive design principles
- Implement proper accessibility features
- Use existing components correctly
- Run build verification after changes

## Technology Stack
- **Framework**: React 19 + TypeScript + Vite
- **Styling**: Tailwind CSS 4.1 with OKLCH color space
- **Components**: Radix UI primitives (via shadcn/ui)
- **Router**: React Router 7.8.2
- **Icons**: Lucide React
- **Tables**: TanStack React Table v8

## Key Component Patterns

### PageHeader Component
All pages should use the `PageHeader` component for consistency:
```typescript
<PageHeader
  breadcrumbs={[{ label: "Parent", href: "/parent" }, { label: "Current" }]}
  title="Page Title"
  description="Optional description"
  actions={<Button>Action</Button>}
/>
```

### Responsive Grids
Always use responsive grid patterns:
```typescript
// ✅ Correct
<div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">

// ❌ Wrong
<div className="grid grid-cols-3 gap-4">
```

### Theme Colors
Use CSS variables, never hardcoded colors:
```typescript
// ✅ Correct
<div className="bg-primary text-primary-foreground">

// ❌ Wrong
<div className="bg-blue-500 text-white">
```

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

### Pattern for Extracted Components

```typescript
// components/devices/platform-assignment.tsx
interface PlatformAssignmentProps {
  uuid: string;
  device: Device;
  assignedPlatform: Platform | null;
  availablePlatforms: Platform[];
  onPlatformUpdate: (platform: Platform | null, device: Device) => void;
  onError: (error: string) => void;
}

export function PlatformAssignment({ uuid, device, ... }: PlatformAssignmentProps) {
  // Component manages its own internal state
  const [selectedPlatformId, setSelectedPlatformId] = useState<number | null>(device.platform_id);
  const [loading, setLoading] = useState(false);

  const handleAssign = async () => {
    setLoading(true);
    try {
      await assignDevicePlatform(uuid, selectedPlatformId);
      const [updatedPlatform, updatedDevice] = await Promise.all([
        getDevicePlatform(uuid),
        getDevice(uuid)
      ]);
      onPlatformUpdate(updatedPlatform, updatedDevice);
    } catch (err) {
      onError(err.message);
    } finally {
      setLoading(false);
    }
  };

  return <Card>...</Card>;
}
```

### Benefits
- **Separation of concerns**: Page manages high-level state, component handles details
- **Testability**: Components can be tested in isolation
- **Reusability**: Components can be used across multiple pages
- **Maintainability**: Easier to understand and modify focused components
- **Performance**: Can optimize re-renders independently

### When to Extract
- ✅ Role Assignment Card (has selection state, API calls)
- ✅ Platform Assignment Card (has selection state, API calls)
- ✅ BMC Configuration (complex form logic)
- ❌ Simple information displays (Device UUID, Architecture)
- ❌ Single API call without state (can stay inline)

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
