import { useState, useEffect } from "react";
import type { DhcpNetwork } from "@/lib/client";
import { getPoolsForNetwork, deleteNetwork } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Eye, Trash2 } from "lucide-react";
import { useNavigate } from "react-router";
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

type NetworkWithPoolCount = DhcpNetwork & {
  poolCount?: number;
};

export default function NetworksTable({ data }: { data: DhcpNetwork[] }) {
  const navigate = useNavigate();
  const [networks, setNetworks] = useState<NetworkWithPoolCount[]>(data);
  const [isDeleting, setIsDeleting] = useState(false);

  useEffect(() => {
    const fetchPoolCounts = async () => {
      const networksWithCounts = await Promise.all(
        data.map(async (network) => {
          try {
            const pools = await getPoolsForNetwork(network.id);
            return { ...network, poolCount: pools.length };
          } catch (error) {
            return { ...network, poolCount: 0 };
          }
        })
      );
      setNetworks(networksWithCounts);
    };
    fetchPoolCounts();
  }, [data]);

  const handleDelete = async (id: number) => {
    setIsDeleting(true);
    try {
      await deleteNetwork(id);
      setNetworks(networks.filter((n) => n.id !== id));
    } catch (error) {
      console.error("Failed to delete network:", error);
    } finally {
      setIsDeleting(false);
    }
  };

  const columns: ColumnDef<NetworkWithPoolCount>[] = [
    {
      accessorKey: "name",
      header: "Name",
      cell: ({ row }) => {
        return (
          <div className="flex items-center gap-2">
            <button
              onClick={() => navigate(`/networks/${row.original.id}`)}
              className="text-primary hover:underline font-medium"
            >
              {row.getValue("name")}
            </button>
          </div>
        );
      },
    },
    {
      accessorKey: "subnet",
      header: "Subnet",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("subnet")}</span>
      ),
    },
    {
      accessorKey: "gateway",
      header: "Gateway",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("gateway")}</span>
      ),
    },
    {
      id: "relay_agent",
      header: "Relay Agent",
      cell: ({ row }) => {
        const relayAgent = row.original.relay_agent_address;
        return relayAgent ? (
          <span className="font-mono text-xs">{relayAgent}</span>
        ) : (
          <span className="text-muted-foreground text-sm">Local L2</span>
        );
      },
    },
    {
      id: "pool_count",
      header: "Pools",
      cell: ({ row }) => {
        const count = row.original.poolCount;
        return count !== undefined ? (
          <span className="text-sm">{count}</span>
        ) : (
          <span className="text-muted-foreground text-sm">Loading...</span>
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
              onClick={() => navigate(`/networks/${row.original.id}`)}
              aria-label="View network details"
            >
              <Eye className="h-4 w-4" />
            </Button>
            <AlertDialog>
              <AlertDialogTrigger asChild>
                <Button
                  variant="outline"
                  size="sm"
                  aria-label="Delete network"
                >
                  <Trash2 className="h-4 w-4" />
                </Button>
              </AlertDialogTrigger>
              <AlertDialogContent>
                <AlertDialogHeader>
                  <AlertDialogTitle>Delete Network</AlertDialogTitle>
                  <AlertDialogDescription>
                    Are you sure you want to delete the network "{row.original.name}"? This will also
                    delete all associated pools and static reservations. This action cannot be undone.
                  </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                  <AlertDialogCancel>Cancel</AlertDialogCancel>
                  <AlertDialogAction
                    onClick={() => handleDelete(row.original.id)}
                    disabled={isDeleting}
                  >
                    {isDeleting ? "Deleting..." : "Delete"}
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
    data: networks,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

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
                No networks found.
              </TableCell>
            </TableRow>
          )}
        </TableBody>
      </Table>
    </div>
  );
}
