import type { Device, DhcpLease } from "@/lib/client"
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table"
import { Button } from "../ui/button"
import { Badge } from "../ui/badge"
import { StatusBadge } from "../ui/status-badge"
import { Eye } from "lucide-react"
import { useNavigate } from "react-router"

interface DevicesTableEnhancedProps {
  data: Device[];
  dhcpLeases: DhcpLease[];
  rolesMap: Map<number, { name: string; os_name: string; os_version: string }>;
  platformsMap: Map<number, { name: string }>;
}

export default function DevicesTableEnhanced({ data, dhcpLeases, rolesMap, platformsMap }: DevicesTableEnhancedProps) {
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
      id: "platform",
      header: "Platform",
      cell: ({ row }) => {
        const platformId = row.original.platform_id;
        if (!platformId) {
          return <span className="text-muted-foreground">Not assigned</span>;
        }

        const platformInfo = platformsMap.get(platformId);
        if (!platformInfo) {
          return <Badge variant="secondary">Platform #{platformId}</Badge>;
        }

        return (
          <button
            onClick={() => navigate(`/platforms/${platformId}`)}
            className="text-blue-600 hover:underline text-sm"
          >
            {platformInfo.name}
          </button>
        );
      },
    },
    {
      id: "lifecycle",
      header: "Status",
      cell: ({ row }) => {
        const lifecycle = row.original.lifecycle;
        return lifecycle ? (
          <StatusBadge status={lifecycle} />
        ) : (
          <span className="text-muted-foreground">—</span>
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
