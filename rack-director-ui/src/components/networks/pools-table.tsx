import { useState } from "react";
import type { DhcpPool, CreateDhcpPoolRequest } from "@/lib/client";
import { createPool, updatePool, deletePool } from "@/lib/client";
import { PoolDialog } from "./pool-dialog";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogTrigger,
} from "@/components/ui/alert-dialog";

interface PoolsTableProps {
  networkId: number;
  pools: DhcpPool[];
  onPoolsChange: (pools: DhcpPool[]) => void;
}

export default function PoolsTable({ networkId, pools, onPoolsChange }: PoolsTableProps) {
  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  const [isEditDialogOpen, setIsEditDialogOpen] = useState(false);
  const [editingPool, setEditingPool] = useState<DhcpPool | null>(null);

  const handleAdd = async (pool: CreateDhcpPoolRequest) => {
    const newPool = await createPool(networkId, pool);
    onPoolsChange([...pools, newPool]);
  };

  const handleEdit = async (pool: CreateDhcpPoolRequest) => {
    if (!editingPool) return;
    const updated = await updatePool(editingPool.id, pool);
    onPoolsChange(pools.map((p) => (p.id === updated.id ? updated : p)));
    setEditingPool(null);
  };

  const handleDelete = async (id: number) => {
    await deletePool(id);
    onPoolsChange(pools.filter((p) => p.id !== id));
  };

  const openEditDialog = (pool: DhcpPool) => {
    setEditingPool(pool);
    setIsEditDialogOpen(true);
  };

  return (
    <div className="space-y-3">
      <div className="flex justify-end">
        <button
          type="button"
          onClick={() => setIsAddDialogOpen(true)}
          className="px-3 py-1 h-7 text-xs font-medium bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover transition-colors cursor-pointer"
        >
          + Add Pool
        </button>
      </div>

      <PoolDialog
        open={isAddDialogOpen}
        onOpenChange={setIsAddDialogOpen}
        onSave={handleAdd}
      />

      <PoolDialog
        open={isEditDialogOpen}
        onOpenChange={setIsEditDialogOpen}
        pool={editingPool}
        onSave={handleEdit}
      />

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["Pool Name", "Range Start", "Range End", ""] as const).map((col, i) => (
                <th
                  key={i}
                  className="text-left text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] px-3 py-2 border-b border-border"
                >
                  {col}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {pools.length === 0 ? (
              <tr>
                <td colSpan={4} className="px-3 py-6 text-center text-xs text-text-muted">
                  No pools defined. Add a pool to allocate IP addresses dynamically.
                </td>
              </tr>
            ) : (
              pools.map((pool, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={pool.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    <td className="px-3 py-2 text-xs text-text-primary font-medium">
                      {pool.name}
                    </td>
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {pool.range_start}
                    </td>
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {pool.range_end}
                    </td>
                    <td className="px-3 py-2">
                      <div className="flex items-center gap-3">
                        <button
                          type="button"
                          onClick={() => openEditDialog(pool)}
                          className="text-xs text-accent hover:text-accent-hover transition-colors cursor-pointer"
                          aria-label={`Edit pool ${pool.name}`}
                        >
                          edit
                        </button>
                        <AlertDialog>
                          <AlertDialogTrigger asChild>
                            <button
                              type="button"
                              className="text-xs text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                              aria-label={`Delete pool ${pool.name}`}
                            >
                              delete
                            </button>
                          </AlertDialogTrigger>
                          <AlertDialogContent>
                            <AlertDialogHeader>
                              <AlertDialogTitle>Delete Pool</AlertDialogTitle>
                              <AlertDialogDescription>
                                Are you sure you want to delete the pool "{pool.name}"? This action
                                cannot be undone.
                              </AlertDialogDescription>
                            </AlertDialogHeader>
                            <AlertDialogFooter>
                              <AlertDialogCancel>Cancel</AlertDialogCancel>
                              <AlertDialogAction onClick={() => handleDelete(pool.id)}>
                                Delete
                              </AlertDialogAction>
                            </AlertDialogFooter>
                          </AlertDialogContent>
                        </AlertDialog>
                      </div>
                    </td>
                  </tr>
                );
              })
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
