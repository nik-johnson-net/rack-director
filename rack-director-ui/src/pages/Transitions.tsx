import { PageHeader } from "@/components/ui/page-header";
import { Card, CardContent } from "@/components/ui/card";

function Transitions() {
  return (
    <div className="space-y-6">
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Transitions" },
        ]}
        title="Transitions"
        description="Device lifecycle transition history"
      />

      <Card>
        <CardContent className="pt-6">
          <p className="text-sm text-text-secondary">
            Transition history coming soon.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}

export default Transitions;
