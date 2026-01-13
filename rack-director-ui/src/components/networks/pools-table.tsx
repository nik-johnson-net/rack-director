import { useState } from "react";
import type { DhcpPool, CreateDhcpPoolRequest } from "@/lib/client";
import { createPool, updatePool, deletePool } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Pencil, Trash2, Plus } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
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
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [formData, setFormData] = useState<CreateDhcpPoolRequest>({
    name: "",
    range_start: "",
    range_end: "",
  });

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      const newPool = await createPool(networkId, formData);
      onPoolsChange([...pools, newPool]);
      setIsAddDialogOpen(false);
      setFormData({ name: "", range_start: "", range_end: "" });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create pool");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleEdit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editingPool) return;

    setError(null);
    setIsSubmitting(true);

    try {
      const updated = await updatePool(editingPool.id, formData);
      onPoolsChange(pools.map((p) => (p.id === updated.id ? updated : p)));
      setIsEditDialogOpen(false);
      setEditingPool(null);
      setFormData({ name: "", range_start: "", range_end: "" });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to update pool");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async (id: number) => {
    setError(null);
    try {
      await deletePool(id);
      onPoolsChange(pools.filter((p) => p.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete pool");
    }
  };

  const openEditDialog = (pool: DhcpPool) => {
    setEditingPool(pool);
    setFormData({
      name: pool.name,
      range_start: pool.range_start,
      range_end: pool.range_end,
    });
    setIsEditDialogOpen(true);
  };

  const columns: ColumnDef<DhcpPool>[] = [
    {
      accessorKey: "name",
      header: "Pool Name",
      cell: ({ row }) => (
        <span className="font-medium">{row.getValue("name")}</span>
      ),
    },
    {
      accessorKey: "range_start",
      header: "Range Start",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("range_start")}</span>
      ),
    },
    {
      accessorKey: "range_end",
      header: "Range End",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("range_end")}</span>
      ),
    },
    {
      id: "actions",
      header: "Actions",
      cell: ({ row }) => {
        return (
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="sm"
              onClick={() => openEditDialog(row.original)}
              aria-label="Edit pool"
            >
              <Pencil className="h-4 w-4" />
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button variant="outline" size="sm" aria-label="Delete pool">
                  <Trash2 className="h-4 w-4" />
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete Pool</AlertDialogTitle>
                  <AlertDialogDescription>
                    Are you sure you want to delete the pool "{row.original.name}"? This action
                    cannot be undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction onClick={() => handleDelete(row.original.id)}>
                    Delete
                  </AlertDialogAction>
                </AlertDialogFooter>
              </AlertDialogContent>
            </AlertDialog>
          </div>
        );
      },
    },
  ];

  const table = useReactTable({
    data: pools,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  const PoolForm = ({ onSubmit }: { onSubmit: (e: React.FormEvent) => void }) => (
    <form onSubmit={onSubmit} className="space-y-4">
      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md text-sm">
          {error}
        </div>
      )}
      <div className="space-y-2">
        <Label htmlFor="pool-name">Pool Name *</Label>
        <Input
          id="pool-name"
          value={formData.name}
          onChange={(e) => setFormData({ ...formData, name: e.target.value })}
          placeholder="e.g., Main Pool"
          required
        />
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="range-start">Range Start *</Label>
          <Input
            id="range-start"
            value={formData.range_start}
            onChange={(e) => setFormData({ ...formData, range_start: e.target.value })}
            placeholder="e.g., 192.168.1.100"
            required
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="range-end">Range End *</Label>
          <Input
            id="range-end"
            value={formData.range_end}
            onChange={(e) => setFormData({ ...formData, range_end: e.target.value })}
            placeholder="e.g., 192.168.1.200"
            required
          />
        </div>
      </div>
      <DialogFooter>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? "Saving..." : editingPool ? "Update Pool" : "Add Pool"}
        </Button>
      </DialogFooter>
    </form>
  );

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <Dialog open={isAddDialogOpen} onOpenChange={setIsAddDialogOpen}>
          <DialogTrigger asChild>
            <Button size="sm">
              <Plus className="h-4 w-4 mr-2" />
              Add Pool
            </Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Add Pool</DialogTitle>
              <DialogDescription>
                Create a new IP address pool for this network.
              </DialogDescription>
            </DialogHeader>
            <PoolForm onSubmit={handleAdd} />
          </DialogContent>
        </Dialog>
      </div>

      <Dialog open={isEditDialogOpen} onOpenChange={setIsEditDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Edit Pool</DialogTitle>
            <DialogDescription>Update the pool configuration.</DialogDescription>
          </DialogHeader>
          <PoolForm onSubmit={handleEdit} />
        </DialogContent>
      </Dialog>

      <div className="overflow-hidden rounded-md border">
        <Table>
          <TableHeader>
            {table.getHeaderGroups().map((headerGroup) => (
              <TableRow key={headerGroup.id}>
                {headerGroup.headers.map((header) => {
                  return (
                    <TableHead key={header.id}>
                      {header.isPlaceholder
                        ? null
                        : flexRender(header.column.columnDef.header, header.getContext())}
                    </TableHead>
                  );
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow key={row.id} data-state={row.getIsSelected() && "selected"}>
                  {row.getVisibleCells().map((cell) => (
                    <TableCell key={cell.id}>
                      {flexRender(cell.column.columnDef.cell, cell.getContext())}
                    </TableCell>
                  ))}
                </TableRow>
              ))
            ) : (
              <TableRow>
                <TableCell colSpan={columns.length} className="h-24 text-center">
                  <div className="text-muted-foreground">
                    No pools defined. Add a pool to allocate IP addresses dynamically.
                  </div>
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>
    </div>
  );
}
