import { PageHeader } from "@/components/ui/page-header";
import { GlobalCloudConfigCard } from "@/components/cloud-config/global-cloud-config-card";

function Settings() {
  return (
    <div className="space-y-6">
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Settings" },
        ]}
        title="Settings"
        description="Application configuration and preferences"
      />

      <GlobalCloudConfigCard />
    </div>
  );
}

export default Settings;
