import { useState } from "react";
import type { Platform } from "@/lib/client"
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table"
import { Button } from "../ui/button"
import { Pencil, Trash2 } from "lucide-react"
import { useNavigate } from "react-router"
import { DeleteConfirmationDialog } from "../ui/delete-confirmation-dialog";

interface PlatformsTableProps {
  data: Platform[];
  onDelete: (id: number) => Promise<void>;
}

export default function PlatformsTable({ data, onDelete }: PlatformsTableProps) {
  const navigate = useNavigate();
  const [deleteDialogOpen, setDeleteDialogOpen] = useState(false);
  const [platformToDelete, setPlatformToDelete] = useState<Platform | null>(null);

  const handleDeleteClick = (platform: Platform) => {
    setPlatformToDelete(platform);
    setDeleteDialogOpen(true);
  };

  const handleConfirmDelete = async () => {
    if (platformToDelete?.id) {
      await onDelete(platformToDelete.id);
      setPlatformToDelete(null);
    }
  };

  const formatHardwareSummary = (platform: Platform): string => {
    const { cpus, memory_gib, disks, nics } = platform.attributes;

    const cpuSummary = cpus.length > 0
      ? `${cpus.length}x ${cpus[0].cores}-core ${cpus[0].brand}`
      : "No CPUs";

    return `${cpuSummary}, ${memory_gib}GB, ${disks.length} disk${disks.length !== 1 ? 's' : ''}, ${nics.length} NIC${nics.length !== 1 ? 's' : ''}`;
  };

  const columns: ColumnDef<Platform>[] = [
    {
      accessorKey: "name",
      header: "Name",
      cell: ({ row }) => {
        return (
          <button
            onClick={() => navigate(`/platforms/${row.original.id}`)}
            className="text-blue-600 hover:underline font-medium"
          >
            {row.getValue("name")}
          </button>
        );
      },
    },
    {
      accessorKey: "description",
      header: "Description",
      cell: ({ row }) => {
        const description = row.getValue("description") as string | undefined;
        return description || <span className="text-muted-foreground">—</span>;
      },
    },
    {
      id: "hardware",
      header: "Hardware Summary",
      cell: ({ row }) => {
        return (
          <span className="text-sm text-muted-foreground">
            {formatHardwareSummary(row.original)}
          </span>
        );
      },
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
              onClick={() => navigate(`/platforms/${row.original.id}/edit`)}
              aria-label="Edit platform"
            >
              <Pencil className="h-4 w-4" />
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => handleDeleteClick(row.original)}
              aria-label="Delete platform"
            >
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        );
      },
    },
  ];

  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <>
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
                        : flexRender(
                            header.column.columnDef.header,
                            header.getContext()
                          )}
                    </TableHead>
                  )
                })}
              </TableRow>
            ))}
          </TableHeader>
          <TableBody>
            {table.getRowModel().rows?.length ? (
              table.getRowModel().rows.map((row) => (
                <TableRow
                  key={row.id}
                  data-state={row.getIsSelected() && "selected"}
                >
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
                  No platforms found.
                </TableCell>
              </TableRow>
            )}
          </TableBody>
        </Table>
      </div>

      <DeleteConfirmationDialog
        open={deleteDialogOpen}
        onOpenChange={setDeleteDialogOpen}
        onConfirm={handleConfirmDelete}
        title="Delete Platform"
        description={`Are you sure you want to delete the platform "${platformToDelete?.name}"? This action cannot be undone.`}
      />
    </>
  );
}
