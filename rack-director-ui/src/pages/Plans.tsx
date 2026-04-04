import { PageHeader } from "@/components/ui/page-header";
import { Card, CardContent } from "@/components/ui/card";

function Plans() {
  return (
    <div className="space-y-6">
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Plans" },
        ]}
        title="Plans"
        description="Provisioning plans and execution history"
      />

      <Card>
        <CardContent className="pt-6">
          <p className="text-sm text-text-secondary">
            Plans coming soon.
          </p>
        </CardContent>
      </Card>
    </div>
  );
}

export default Plans;
