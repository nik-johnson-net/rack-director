import NetworksTable from "@/components/networks/networks-table";
import type { DhcpNetwork } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { Plus } from "lucide-react";
import { PageHeader } from "@/components/ui/page-header";

function Networks() {
  const data = useLoaderData<DhcpNetwork[]>();
  const navigate = useNavigate();

  return (
    <div className="space-y-6 max-w-5xl">
      <PageHeader
        breadcrumbs={[{ label: "Networks" }]}
        title="DHCP Networks"
        description="Manage DHCP networks, pools, and static reservations"
        actions={
          <Button onClick={() => navigate('/networks/new')}>
            <Plus className="h-4 w-4 mr-2" />
            Add Network
          </Button>
        }
      />
      <NetworksTable data={data} />
    </div>
  );
}

export default Networks;
