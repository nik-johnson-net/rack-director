import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { PageHeader } from "@/components/ui/page-header";
import { createOperatingSystem } from "@/lib/client";

/* Shared label style matching the dark terminal design */
const labelCls =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";
const inputCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

function OperatingSystemNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      const os = await createOperatingSystem({ name, version });
      navigate(`/operating-systems/${os.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create operating system");
      setIsSubmitting(false);
    }
  };

  return (
    <div className="max-w-2xl">
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "OS Images", href: "/operating-systems" },
          { label: "New" },
        ]}
        title="Add Operating System"
        description="Create a new OS image and configure its architectures after creation"
      />

      {/* Card */}
      <div className="bg-bg-surface border border-border p-4">
        <p className="text-xs font-semibold text-text-primary mb-4">Basic Information</p>

        <form onSubmit={handleSubmit}>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-4 mb-4">
            {/* Name */}
            <div>
              <label htmlFor="name" className={labelCls}>
                Name <span className="text-accent">*</span>
              </label>
              <Input
                id="name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g., Ubuntu"
                required
                className={inputCls}
              />
            </div>

            {/* Version */}
            <div>
              <label htmlFor="version" className={labelCls}>
                Version <span className="text-accent">*</span>
              </label>
              <Input
                id="version"
                value={version}
                onChange={(e) => setVersion(e.target.value)}
                placeholder="e.g., 22.04"
                required
                className={inputCls}
              />
            </div>
          </div>

          {error && (
            <div className="bg-error-bg border border-error-border text-status-broken px-3 py-2 text-xs mb-4">
              {error}
            </div>
          )}

          <div className="flex gap-2">
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? "Creating..." : "Create Operating System"}
            </Button>
            <Button
              type="button"
              variant="outline"
              onClick={() => navigate("/operating-systems")}
            >
              Cancel
            </Button>
          </div>
        </form>
      </div>
    </div>
  );
}

export default OperatingSystemNew;
