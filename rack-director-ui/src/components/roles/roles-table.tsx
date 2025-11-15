import type { RoleWithOs } from "@/lib/client"
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table"
import { Button } from "../ui/button"
import { Badge } from "../ui/badge"
import { Pencil } from "lucide-react"
import { useNavigate } from "react-router"

const columns: ColumnDef<RoleWithOs>[] = [
  {
    accessorKey: "name",
    header: "Name",
    cell: ({ row }) => {
      const navigate = useNavigate();
      return (
        <button
          onClick={() => navigate(`/roles/${row.original.id}`)}
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
      return description || <span className="text-gray-400">—</span>;
    },
  },
  {
    id: "operating_system",
    header: "Operating System",
    cell: ({ row }) => {
      return (
        <Badge variant="outline">
          {row.original.os_name} {row.original.os_version}
        </Badge>
      );
    },
  },
  {
    id: "partitions",
    header: "Partitions",
    cell: ({ row }) => {
      const count = row.original.disk_layout.partitions.length;
      return <span className="text-sm text-gray-600">{count} partition{count !== 1 ? 's' : ''}</span>;
    },
  },
  {
    id: "actions",
    header: "Actions",
    cell: ({ row }) => {
      const navigate = useNavigate();
      return (
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => navigate(`/roles/${row.original.id}`)}
          >
            <Pencil className="h-4 w-4" />
          </Button>
        </div>
      );
    },
  },
]

export default function RolesTable({ data }: { data: RoleWithOs[] }) {
  const table = useReactTable({
    data,
    columns,
    getCoreRowModel: getCoreRowModel(),
  })

  return (
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
                No roles found.
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
