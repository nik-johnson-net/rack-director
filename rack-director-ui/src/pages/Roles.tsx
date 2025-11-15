import RolesTable from "@/components/roles/roles-table";
import type { RoleWithOs } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router"
import { Button } from "@/components/ui/button";
import { Plus } from "lucide-react";

function Roles() {
  const data = useLoaderData<RoleWithOs[]>();
  const navigate = useNavigate();

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h1 className="text-3xl font-bold">Roles</h1>
        <Button onClick={() => navigate('/roles/new')}>
          <Plus className="h-4 w-4 mr-2" />
          Add Role
        </Button>
      </div>
      <RolesTable data={data} />
    </div>
  )
}

export default Roles
