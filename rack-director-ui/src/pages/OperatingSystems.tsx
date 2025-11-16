import OperatingSystemsTable from "@/components/operating-systems/operating-systems-table";
import type { OperatingSystem } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router"
import { Button } from "@/components/ui/button";
import { Plus } from "lucide-react";

function OperatingSystems() {
  const data = useLoaderData<OperatingSystem[]>();
  const navigate = useNavigate();

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h1 className="text-3xl font-bold">Operating Systems</h1>
        <Button onClick={() => navigate('/operating-systems/new')}>
          <Plus className="h-4 w-4 mr-2" />
          Add Operating System
        </Button>
      </div>
      <OperatingSystemsTable data={data} />
    </div>
  )
}

export default OperatingSystems
