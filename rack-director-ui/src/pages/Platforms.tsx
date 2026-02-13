import { useState } from "react";
import PlatformsTable from "@/components/platforms/platforms-table";
import type { Platform } from "@/lib/client";
import { useLoaderData, useNavigate } from "react-router"
import { Button } from "@/components/ui/button";
import { Plus } from "lucide-react";
import { deletePlatform } from "@/lib/client";

function Platforms() {
  const initialData = useLoaderData<Platform[]>();
  const navigate = useNavigate();
  const [data, setData] = useState(initialData);
  const [error, setError] = useState<string | null>(null);

  const handleDelete = async (id: number) => {
    setError(null);
    try {
      await deletePlatform(id);
      // Remove from local state
      setData(data.filter(p => p.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete platform");
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h1 className="text-3xl font-bold">Platforms</h1>
        <Button onClick={() => navigate('/platforms/new')}>
          <Plus className="h-4 w-4 mr-2" />
          Add Platform
        </Button>
      </div>

      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md">
          {error}
        </div>
      )}

      <PlatformsTable data={data} onDelete={handleDelete} />
    </div>
  )
}

export default Platforms
