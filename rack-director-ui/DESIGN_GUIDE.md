# Rack Director UI Design Guide

## Purpose
This specification defines the guidelines and patterns for making UI changes to Rack Director UI. All developers and agents should follow these principles to maintain consistency, quality, and user experience.

## Project Context

### Technology Stack
- **Framework**: React 19 + TypeScript + Vite
- **Styling**: Tailwind CSS 4.1 with OKLCH color space
- **Components**: Radix UI primitives with class-variance-authority (CVA)
- **Router**: React Router 7.8.2
- **Icons**: Lucide React
- **Tables**: TanStack React Table v8

### Design System
- **Theme**: Modern Enterprise with blue/cyan brand colors
- **Color Mode**: Light-first approach with refined dark mode
- **Color Space**: OKLCH for perceptual uniformity
- **Border Radius**: 8px (0.5rem) base for refined modern aesthetic

## Core Design Principles

### 1. Consistency
**Objective**: Create a predictable, cohesive user experience across all pages.

**Rules:**
- Use `PageHeader` component on all pages for consistent page structure
- Follow spacing scale: 4px, 8px, 16px, 24px, 32px, 48px, 64px (Tailwind: `1`, `2`, `4`, `6`, `8`, `12`, `16`)
- Standardized card padding: `p-6` for CardContent
- Uniform button sizes: `sm`, `default`, `lg`, `icon`
- Max-width containers:
  - Forms: `max-w-2xl`
  - Medium content: `max-w-4xl`
  - Data tables: `max-w-5xl`
  - Dashboards: `max-w-7xl`

**Component Reuse:**
```typescript
// Always use existing components
import { PageHeader } from "@/components/ui/page-header";
import { Breadcrumbs } from "@/components/ui/breadcrumbs";
import { StatusBadge } from "@/components/ui/status-badge";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
```

### 2. Clarity
**Objective**: Make information hierarchy immediately obvious.

**Rules:**
- **Heading Hierarchy**:
  - h1: Page title (text-3xl font-bold)
  - h2: Section titles (text-xl font-semibold)
  - h3: Subsection titles (text-base font-medium)
- **Visual Weight**: Use size, font-weight, and color to create hierarchy
- **Labels**: Always descriptive, never ambiguous
- **Placeholders**: Provide examples (e.g., "e.g., /dev/disk/by-path/pci-0000:00:1f.2-ata-1" not just "Device path")
- **Empty States**: Use `EmptyState` component with:
  - Large icon (muted color)
  - Clear heading explaining the empty state
  - Helpful description
  - Primary CTA button to resolve

**Typography:**
```typescript
// Page title
<h1 className="text-3xl font-bold tracking-tight">Title</h1>

// Section title
<h2 className="text-xl font-semibold">Section</h2>

// Muted text
<p className="text-muted-foreground">Supporting text</p>

// Technical data (UUIDs, IPs, MACs)
<span className="font-mono text-xs">00:11:22:33:44:55</span>
```

### 3. Efficiency
**Objective**: Minimize user effort to accomplish tasks.

**Rules:**
- **Progressive Disclosure**: Use tabs, accordions, or expandable sections for complex content
- **Quick Actions**: Place primary actions in page header
- **Shortcuts**: Provide keyboard shortcuts where appropriate
- **Smart Defaults**: Pre-fill form fields when context is available
- **Inline Actions**: Edit/delete actions near the item, not requiring navigation

**Example Patterns:**
```typescript
// Tabs for complex detail pages
<Tabs defaultValue="overview">
  <TabsList>
    <TabsTrigger value="overview">Overview</TabsTrigger>
    <TabsTrigger value="network">Network</TabsTrigger>
  </TabsList>
</Tabs>

// Quick actions in page header
<PageHeader
  title="Device Details"
  actions={
    <>
      <Button variant="outline">Reboot</Button>
      <Button>Provision</Button>
    </>
  }
/>
```

### 4. Responsiveness
**Objective**: Ensure excellent experience on all device sizes.

**Rules:**
- **Mobile-First**: Design components for mobile, enhance for desktop
- **Breakpoints**: Test at 375px (mobile), 768px (tablet), 1024px (small desktop), 1440px (desktop)
- **Grid Responsiveness**: Always use responsive grids
  ```typescript
  // ✅ Correct
  <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">

  // ❌ Wrong
  <div className="grid grid-cols-3 gap-4">
  ```
- **Touch Targets**: Minimum 44×44px for interactive elements on mobile
- **Text Size**: Minimum 14px (text-sm), never smaller
- **Tables**: Use responsive table wrapper or card view on mobile

**Responsive Patterns:**
```typescript
// Responsive grid
<div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-4">

// Responsive flex
<div className="flex flex-col sm:flex-row gap-4">

// Hide on mobile
<div className="hidden sm:block">Desktop only</div>

// Mobile only
<div className="sm:hidden">Mobile only</div>
```

## Theme & Styling Guidelines

### Color Usage

**Semantic Colors (use CSS variables):**
```typescript
// Backgrounds
"bg-background"         // Page background
"bg-card"              // Card background
"bg-muted"             // Muted background
"bg-secondary"         // Secondary surfaces

// Text
"text-foreground"      // Primary text
"text-muted-foreground" // Secondary text
"text-primary"         // Brand color text

// Interactive
"bg-primary"           // Primary buttons, active states
"hover:bg-primary/90"  // Hover states
"border-border"        // Standard borders
"ring-ring"            // Focus rings
```

**Status Colors:**
```typescript
// Use StatusBadge component for device states
<StatusBadge status="provisioned" />  // Green
<StatusBadge status="unprovisioned" /> // Amber
<StatusBadge status="new" />          // Blue
<StatusBadge status="broken" />       // Red
<StatusBadge status="removed" />      // Gray
```

**Never use hardcoded colors:**
```typescript
// ❌ Wrong
<div className="bg-blue-500 text-white">

// ✅ Correct
<div className="bg-primary text-primary-foreground">
```

### Spacing Scale

**Follow Tailwind spacing:**
- `gap-1` (4px): Tight inline elements
- `gap-2` (8px): Related items
- `gap-4` (16px): Standard spacing (most common)
- `gap-6` (24px): Section separation
- `gap-8` (32px): Major section breaks
- `space-y-4`: Vertical spacing in stacks

### Border Radius

**Use theme tokens:**
```typescript
"rounded-md"    // 8px - standard (buttons, inputs)
"rounded-lg"    // 12px - cards
"rounded-full"  // Badges, avatars
```

## Component Patterns

### Page Structure

**Standard Page Layout:**
```typescript
export default function PageName() {
  return (
    <div className="space-y-6 max-w-5xl">
      <PageHeader
        breadcrumbs={[
          { label: "Parent", href: "/parent" },
          { label: "Current Page" }
        ]}
        title="Page Title"
        description="Brief description of page purpose"
        actions={<Button>Primary Action</Button>}
      />

      {/* Page content */}
      <Card>
        <CardHeader>
          <CardTitle>Section Title</CardTitle>
        </CardHeader>
        <CardContent>
          {/* Content */}
        </CardContent>
      </Card>
    </div>
  );
}
```

### Forms

**Always use the FormField component for form inputs:**

```typescript
import { FormField, FormTextareaField, FormSelectField } from "@/components/ui/form-field";
import { useFieldErrors } from "@/hooks/useFieldErrors";

function MyForm() {
  const { clearFieldError, getError } = useFieldErrors();

  return (
    <FormField
      id="field_name"
      label="Field Label"
      required
      value={value}
      onChange={setValue}
      placeholder="e.g., example"
      helperText="Description of what this field does"
      error={getError("field_name")}
      onClearError={() => clearFieldError("field_name")}
    />
  );
}
```

**FormField Variants:**

```typescript
// Text Input
<FormField
  id="name"
  label="Name"
  required
  value={name}
  onChange={setName}
  placeholder="e.g., John Doe"
  helperText="Enter your full name"
  error={getError("name")}
  onClearError={() => clearFieldError("name")}
/>

// Number Input
<FormField
  id="port"
  label="Port"
  type="number"
  value={port}
  onChange={setPort}
  placeholder="e.g., 8080"
/>

// Textarea
<FormTextareaField
  id="description"
  label="Description"
  value={description}
  onChange={setDescription}
  placeholder="Enter a description"
  rows={4}
  helperText="Provide a detailed description"
/>

// Select
<FormSelectField
  id="role"
  label="Role"
  required
  value={roleId}
  onChange={setRoleId}
  options={[
    { value: "1", label: "Admin" },
    { value: "2", label: "User" }
  ]}
  helperText="Select a role for this user"
/>
```

**Complete Form Structure:**
```typescript
<form onSubmit={handleSubmit} className="space-y-6">
  <Card>
    <CardHeader>
      <CardTitle>Form Section</CardTitle>
      <CardDescription>Description of what this section does</CardDescription>
    </CardHeader>
    <CardContent className="space-y-4">
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <FormField
          id="field"
          label="Field Label"
          required
          value={value}
          onChange={setValue}
          placeholder="e.g., example value"
          helperText="Helper text"
          error={getError("field")}
          onClearError={() => clearFieldError("field")}
        />
      </div>
    </CardContent>
  </Card>

  <div className="flex justify-end gap-2">
    <Button type="button" variant="outline" onClick={onCancel}>
      Cancel
    </Button>
    <Button type="submit">Submit</Button>
  </div>
</form>
```

### Tables

**Table Pattern:**
```typescript
<div className="overflow-hidden rounded-md border">
  <Table>
    <TableHeader>
      <TableRow>
        <TableHead>Column 1</TableHead>
        <TableHead>Column 2</TableHead>
        <TableHead>Actions</TableHead>
      </TableRow>
    </TableHeader>
    <TableBody>
      {items.map((item) => (
        <TableRow key={item.id}>
          <TableCell>{item.name}</TableCell>
          <TableCell>
            <StatusBadge status={item.status} />
          </TableCell>
          <TableCell>
            <Button variant="outline" size="sm">
              <Eye className="h-4 w-4" />
            </Button>
          </TableCell>
        </TableRow>
      ))}
    </TableBody>
  </Table>
</div>
```

### Navigation

**Breadcrumbs (required on detail/edit pages):**
```typescript
<Breadcrumbs
  items={[
    { label: "Devices", href: "/devices" },
    { label: device.uuid }
  ]}
/>
```

**Sidebar Navigation:**
- Use semantic icons (Server for Devices, HardDrive for OS, Users for Roles)
- Group related items under section labels
- Active state automatically handled by router

### Modals / Dialogs

**Component Structure:**
- Extract modals into `components/<domain>/<name>-dialog.tsx`
- Use controlled open/close pattern: `open` + `onOpenChange` props
- Handle loading states within the modal
- Reset form state when modal opens (using `useEffect` with `open` dependency)
- Modal components should accept async callbacks for actions

**Generic Modal Components:**
- `DeleteConfirmationDialog` - for any delete confirmation
- `MakeStaticDialog` - for making DHCP leases static (networks domain)
- `PoolDialog` - for creating/editing IP pools (networks domain)
- `TransitionDialog` - for device lifecycle transitions (devices domain)

**Example Modal Component:**
```typescript
import { useState, useEffect } from "react";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogFooter } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";

interface MyDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  data: DataType | null;
  onConfirm: (value: string) => Promise<void>;
}

export function MyDialog({ open, onOpenChange, data, onConfirm }: MyDialogProps) {
  const [value, setValue] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  // Reset form when dialog opens
  useEffect(() => {
    if (open && data) {
      setValue(data.defaultValue);
    }
  }, [open, data]);

  const handleConfirm = async () => {
    setIsLoading(true);
    try {
      await onConfirm(value);
      onOpenChange(false);
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Dialog Title</DialogTitle>
        </DialogHeader>
        <FormField
          id="field"
          label="Field Label"
          value={value}
          onChange={setValue}
        />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={isLoading}>
            Cancel
          </Button>
          <Button onClick={handleConfirm} disabled={isLoading}>
            {isLoading ? "Saving..." : "Save"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
```

**Usage in Parent Component:**
```typescript
const [dialogOpen, setDialogOpen] = useState(false);
const [selectedData, setSelectedData] = useState<DataType | null>(null);

const handleOpenDialog = (data: DataType) => {
  setSelectedData(data);
  setDialogOpen(true);
};

const handleConfirm = async (value: string) => {
  await saveData(value);
  setSelectedData(null);
};

return (
  <>
    <Button onClick={() => handleOpenDialog(data)}>Open Dialog</Button>
    <MyDialog
      open={dialogOpen}
      onOpenChange={setDialogOpen}
      data={selectedData}
      onConfirm={handleConfirm}
    />
  </>
);
```

## Accessibility Requirements

### Mandatory Practices

1. **Semantic HTML:**
   ```typescript
   // ✅ Correct
   <button onClick={handleClick}>Click Me</button>

   // ❌ Wrong
   <div onClick={handleClick}>Click Me</div>
   ```

2. **Keyboard Navigation:**
   - All interactive elements must be keyboard accessible
   - Tab order must be logical
   - Esc key should close modals/dialogs
   - Enter should submit forms

3. **ARIA Labels:**
   ```typescript
   // Icon-only buttons
   <Button variant="outline" size="icon" aria-label="Close dialog">
     <X className="h-4 w-4" />
   </Button>

   // Form validation
   <Input
     aria-invalid={hasError}
     aria-describedby={hasError ? "error-message" : undefined}
   />
   {hasError && <span id="error-message">{error}</span>}
   ```

4. **Focus Management:**
   - Focus indicators must be visible (already handled by theme)
   - Focus should move logically through the page
   - Modals should trap focus

5. **Color Contrast:**
   - Must meet WCAG AA standards (4.5:1 for normal text, 3:1 for large text)
   - Status indicators should not rely on color alone (use icons + text)

6. **Alt Text:**
   ```typescript
   // Images need alt text
   <img src={src} alt="Description of image content" />

   // Decorative images
   <img src={src} alt="" role="presentation" />
   ```

## Testing Requirements

### Before Submitting Changes

1. **Build Verification:**
   ```bash
   npm run build
   ```
   Must complete without errors

2. **Visual Testing:**
   - Test in light mode
   - Test in dark mode
   - Test at mobile width (375px)
   - Test at tablet width (768px)
   - Test at desktop width (1440px)

3. **Accessibility Check:**
   - All interactive elements are keyboard accessible
   - Tab order is logical
   - Focus indicators are visible
   - Color contrast meets WCAG AA

4. **Browser Testing:**
   - Chrome/Edge (primary)
   - Firefox
   - Safari (if available)

## Common Patterns & Solutions

### Loading States

```typescript
// Skeleton loaders
import { Skeleton } from "@/components/ui/skeleton";

{isLoading ? (
  <Skeleton className="h-10 w-full" />
) : (
  <div>{content}</div>
)}

// Button loading state
<Button disabled={isLoading}>
  {isLoading ? "Loading..." : "Submit"}
</Button>
```

### Empty States

```typescript
{items.length === 0 ? (
  <div className="text-center py-12">
    <Server className="h-12 w-12 mx-auto text-muted-foreground mb-4" />
    <h3 className="text-lg font-semibold mb-2">No devices found</h3>
    <p className="text-muted-foreground mb-4">
      Get started by adding your first device.
    </p>
    <Button>Add Device</Button>
  </div>
) : (
  <Table>...</Table>
)}
```

### Error States

```typescript
{error && (
  <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
    <p className="font-medium">Error</p>
    <p className="text-sm">{error.message}</p>
  </div>
)}
```

### Copy to Clipboard

```typescript
import { Copy } from "lucide-react";

<div className="flex items-center gap-2">
  <span className="font-mono text-xs">{uuid}</span>
  <Button
    variant="ghost"
    size="icon"
    onClick={() => navigator.clipboard.writeText(uuid)}
    aria-label="Copy UUID"
  >
    <Copy className="h-3 w-3" />
  </Button>
</div>
```

## Anti-Patterns (Avoid These)

### ❌ Don't Do This

```typescript
// Hardcoded colors
<div className="bg-blue-500">

// Non-responsive grids
<div className="grid grid-cols-3">

// Inline styles
<div style={{ color: 'red' }}>

// Non-semantic elements
<div onClick={handleClick}>Click</div>

// Missing labels
<Input placeholder="Name" /> // No label!

// Generic text
<span className="text-gray-400">—</span> // Use text-muted-foreground

// Absolute units
<div style={{ width: '500px' }}> // Use Tailwind utilities

// Missing error handling
<Input value={value} /> // What if value is undefined?
```

### ✅ Do This Instead

```typescript
// Theme colors
<div className="bg-primary">

// Responsive grids
<div className="grid grid-cols-1 md:grid-cols-3">

// Tailwind utilities
<div className="text-destructive">

// Semantic elements
<button onClick={handleClick}>Click</button>

// Proper labels
<Label htmlFor="name">Name</Label>
<Input id="name" placeholder="e.g., John Doe" />

// Theme utilities
<span className="text-muted-foreground">—</span>

// Tailwind utilities
<div className="max-w-md"> // Or max-w-2xl, max-w-5xl

// Defensive coding
<Input value={value ?? ''} />
```

## File Organization

### Component Location

```
src/
├── components/
│   ├── ui/              # Reusable UI primitives
│   │   ├── button.tsx
│   │   ├── card.tsx
│   │   ├── breadcrumbs.tsx
│   │   ├── page-header.tsx
│   │   └── status-badge.tsx
│   │
│   ├── devices/         # Device-specific components
│   │   └── devices-table-enhanced.tsx
│   │
│   └── roles/           # Role-specific components
│       └── partition-editor.tsx
│
├── pages/               # Route components
│   ├── DeviceDetail.tsx
│   └── Devices.tsx
│
└── lib/                 # Utilities
    ├── utils.ts
    └── client.ts
```

### Naming Conventions

- **Components**: PascalCase (`PageHeader.tsx`, `StatusBadge.tsx`)
- **Utilities**: camelCase (`lifecycle-utils.ts`, `utils.ts`)
- **CSS files**: kebab-case (`index.css`, `layout.css`)
- **Types**: PascalCase with `Type` or `Interface` suffix when not component props

## Workflow for New Features

### 1. Planning Phase
- Understand the requirement
- Check existing components that can be reused
- Identify which design patterns apply
- Sketch the component structure

### 2. Implementation Phase
- Create component following patterns above
- Use existing UI components from `components/ui/`
- Apply responsive utilities from the start
- Add proper TypeScript types
- Include accessibility attributes

### 3. Testing Phase
- Build the project (`npm run build`)
- Test responsive behavior (mobile, tablet, desktop)
- Test light and dark modes
- Test keyboard navigation
- Verify color contrast

### 4. Review Phase
- Self-review against this specification
- Check for anti-patterns
- Ensure consistency with existing UI
- Verify accessibility compliance

## Quick Reference Checklist

Before submitting UI changes, verify:

- [ ] Uses existing UI components from `components/ui/`
- [ ] Follows responsive grid patterns (`grid-cols-1 md:grid-cols-2`)
- [ ] Uses theme colors (no hardcoded colors)
- [ ] Includes breadcrumbs on detail/edit pages
- [ ] Has proper heading hierarchy (h1 → h2 → h3)
- [ ] Labels on all form inputs
- [ ] Placeholder text is helpful (e.g., examples)
- [ ] Empty states have clear CTAs
- [ ] Error states are user-friendly
- [ ] Loading states show appropriate feedback
- [ ] Keyboard accessible (tab order, enter, escape)
- [ ] ARIA labels on icon-only buttons
- [ ] Focus indicators visible
- [ ] Tested at mobile width (375px)
- [ ] Tested in dark mode
- [ ] Build completes without errors

---

## Example: Complete Feature Implementation

Here's a complete example showing all principles in action:

```typescript
// src/pages/NewFeature.tsx
import { useState } from "react";
import { useNavigate } from "react-router";
import { PageHeader } from "@/components/ui/page-header";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { StatusBadge } from "@/components/ui/status-badge";

export default function NewFeature() {
  const navigate = useNavigate();
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);

    try {
      // API call
      await saveData();
      navigate("/success");
    } catch (err) {
      setError(err instanceof Error ? err.message : "An error occurred");
    } finally {
      setIsLoading(false);
    }
  };

  return (
    <div className="space-y-6 max-w-5xl">
      {/* Breadcrumbs for context */}
      <PageHeader
        breadcrumbs={[
          { label: "Features", href: "/features" },
          { label: "New Feature" }
        ]}
        title="New Feature"
        description="Create and configure a new feature"
        actions={
          <Button variant="outline" onClick={() => navigate("/features")}>
            Cancel
          </Button>
        }
      />

      {/* Error display */}
      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
          {error}
        </div>
      )}

      {/* Tabbed content for complexity */}
      <Tabs defaultValue="basic">
        <TabsList>
          <TabsTrigger value="basic">Basic Info</TabsTrigger>
          <TabsTrigger value="advanced">Advanced</TabsTrigger>
        </TabsList>

        <TabsContent value="basic" className="space-y-4">
          <form onSubmit={handleSubmit} className="space-y-4">
            <Card>
              <CardHeader>
                <CardTitle>Basic Information</CardTitle>
              </CardHeader>
              <CardContent className="space-y-4">
                {/* Responsive grid */}
                <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
                  <div className="space-y-2">
                    <Label htmlFor="name">Name *</Label>
                    <Input
                      id="name"
                      placeholder="e.g., My Feature"
                      required
                    />
                  </div>
                  <div className="space-y-2">
                    <Label htmlFor="status">Status</Label>
                    <StatusBadge status="new" />
                  </div>
                </div>
              </CardContent>
            </Card>

            {/* Actions */}
            <div className="flex justify-end gap-2">
              <Button
                type="button"
                variant="outline"
                onClick={() => navigate("/features")}
              >
                Cancel
              </Button>
              <Button type="submit" disabled={isLoading}>
                {isLoading ? "Saving..." : "Save"}
              </Button>
            </div>
          </form>
        </TabsContent>

        <TabsContent value="advanced">
          <Card>
            <CardHeader>
              <CardTitle>Advanced Settings</CardTitle>
            </CardHeader>
            <CardContent>
              <p className="text-muted-foreground">
                Advanced configuration options...
              </p>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
```

This example demonstrates:
- ✅ PageHeader with breadcrumbs
- ✅ Responsive grid layout
- ✅ Theme colors (no hardcoded)
- ✅ Proper form structure with labels
- ✅ Error handling
- ✅ Loading states
- ✅ Progressive disclosure (tabs)
- ✅ Consistent spacing
- ✅ Accessibility (labels, keyboard nav)
- ✅ TypeScript types
