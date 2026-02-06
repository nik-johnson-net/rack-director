import { useState } from "react";
import type { DhcpPool, CreateDhcpPoolRequest } from "@/lib/client";
import { createPool, updatePool, deletePool } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Pencil, Trash2, Plus } from "lucide-react";
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
import { PoolDialog } from "./pool-dialog";

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

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <Button size="sm" onClick={() => setIsAddDialogOpen(true)}>
          <Plus className="h-4 w-4 mr-2" />
          Add Pool
        </Button>
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
