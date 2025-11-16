import type { Device, DhcpLease } from "@/lib/client"
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table"
import { Button } from "../ui/button"
import { Badge } from "../ui/badge"
import { Eye } from "lucide-react"
import { useNavigate } from "react-router"

const getLifecycleColor = (lifecycle?: string) => {
  switch (lifecycle) {
    case "provisioned": return "bg-green-100 text-green-800 border-green-300";
    case "unprovisioned": return "bg-yellow-100 text-yellow-800 border-yellow-300";
    case "new": return "bg-blue-100 text-blue-800 border-blue-300";
    case "removed": return "bg-gray-100 text-gray-800 border-gray-300";
    case "broken": return "bg-red-100 text-red-800 border-red-300";
    default: return "bg-gray-100 text-gray-600 border-gray-300";
  }
};

interface DevicesTableEnhancedProps {
  data: Device[];
  dhcpLeases: DhcpLease[];
  rolesMap: Map<number, { name: string; os_name: string; os_version: string }>;
}

export default function DevicesTableEnhanced({ data, dhcpLeases, rolesMap }: DevicesTableEnhancedProps) {
  const navigate = useNavigate();

  // Create MAC to IP mapping
  const macToIp = new Map(dhcpLeases.map(lease => [lease.mac_address, lease.ip_address]));

  const columns: ColumnDef<Device>[] = [
    {
      accessorKey: "uuid",
      header: "UUID",
      cell: ({ row }) => {
        return (
          <button
            onClick={() => navigate(`/devices/${row.original.uuid}`)}
            className="text-blue-600 hover:underline font-mono text-xs"
          >
            {row.getValue("uuid")}
          </button>
        );
      },
    },
    {
      id: "hostname",
      header: "Hostname",
      cell: ({ row }) => {
        const hostname = row.original.attributes?.hostname;
        return hostname || <span className="text-gray-400">—</span>;
      },
    },
    {
      id: "ip_address",
      header: "IP Address",
      cell: ({ row }) => {
        const mac = row.original.attributes?.mac_address;
        const staticIp = row.original.attributes?.static_ip;
        const dhcpIp = mac ? macToIp.get(mac) : null;
        const ip = staticIp || dhcpIp;

        return ip ? (
          <span className="font-mono text-sm">{ip}</span>
        ) : (
          <span className="text-gray-400">—</span>
        );
      },
    },
    {
      accessorKey: "architecture",
      header: "Architecture",
      cell: ({ row }) => {
        return (
          <Badge variant="outline" className="font-mono text-xs">
            {row.getValue("architecture")}
          </Badge>
        );
      },
    },
    {
      id: "role",
      header: "Role",
      cell: ({ row }) => {
        const roleId = row.original.role_id;
        if (!roleId) {
          return <span className="text-gray-400">Not assigned</span>;
        }

        const roleInfo = rolesMap.get(roleId);
        if (!roleInfo) {
          return <Badge variant="secondary">Role #{roleId}</Badge>;
        }

        return (
          <div className="flex flex-col gap-1">
            <Badge variant="secondary" className="text-xs w-fit">
              {roleInfo.name}
            </Badge>
            <span className="text-xs text-gray-500">
              {roleInfo.os_name} {roleInfo.os_version}
            </span>
          </div>
        );
      },
    },
    {
      id: "lifecycle",
      header: "Status",
      cell: ({ row }) => {
        const lifecycle = row.original.lifecycle;
        return lifecycle ? (
          <Badge variant="outline" className={`${getLifecycleColor(lifecycle)} text-xs`}>
            {lifecycle}
          </Badge>
        ) : (
          <span className="text-gray-400">—</span>
        );
      },
    },
    {
      id: "actions",
      header: "Actions",
      cell: ({ row }) => {
        return (
          <Button
            variant="outline"
            size="sm"
            onClick={() => navigate(`/devices/${row.original.uuid}`)}
          >
            <Eye className="h-4 w-4" />
          </Button>
        );
      },
    },
  ];

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
                No devices found.
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  )
}
