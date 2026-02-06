import { useState } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { PageHeader } from "@/components/ui/page-header";
import { FormField, FormTextareaField } from "@/components/ui/form-field";
import { createOperatingSystem } from "@/lib/client";

function OperatingSystemNew() {
  const navigate = useNavigate();
  const [name, setName] = useState("");
  const [version, setVersion] = useState("");
  const [description, setDescription] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      const os = await createOperatingSystem({
        name,
        version,
        description: description || undefined,
      });

      // Redirect to edit page to add architectures
      navigate(`/operating-systems/${os.id}`);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create operating system");
      setIsSubmitting(false);
    }
  };

  return (
    <div className="space-y-4 max-w-2xl">
      <PageHeader
        breadcrumbs={[
          { label: "Operating Systems", href: "/operating-systems" },
          { label: "New Operating System" }
        ]}
        title="Add Operating System"
        description="Create a new operating system and configure its architectures"
      />

      <Card>
        <CardHeader>
          <CardTitle>Basic Information</CardTitle>
          <CardDescription>
            Enter the basic details for the operating system. You can add architectures and upload files after creation.
          </CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit} className="space-y-4">
            <FormField
              id="name"
              label="Name"
              required
              value={name}
              onChange={setName}
              placeholder="e.g., Ubuntu"
            />

            <FormField
              id="version"
              label="Version"
              required
              value={version}
              onChange={setVersion}
              placeholder="e.g., 22.04"
            />

            <FormTextareaField
              id="description"
              label="Description"
              value={description}
              onChange={setDescription}
              placeholder="Optional description"
              rows={3}
            />

            {error && (
              <div className="bg-red-50 border border-red-200 text-red-800 px-4 py-3 rounded">
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
                onClick={() => navigate('/operating-systems')}
              >
                Cancel
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
    </div>
  );
}

export default OperatingSystemNew;
