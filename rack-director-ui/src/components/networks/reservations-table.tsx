import { useState } from "react";
import type { StaticReservation, CreateStaticReservationRequest } from "@/lib/client";
import { createStaticReservation, deleteStaticReservation } from "@/lib/client";
import { flexRender, getCoreRowModel, useReactTable, type ColumnDef } from "@tanstack/react-table";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../ui/table";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { Label } from "../ui/label";
import { Trash2, Plus } from "lucide-react";
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

interface ReservationsTableProps {
  networkId: number;
  reservations: StaticReservation[];
  onReservationsChange: (reservations: StaticReservation[]) => void;
}

export default function ReservationsTable({
  networkId,
  reservations,
  onReservationsChange,
}: ReservationsTableProps) {
  const [isAddDialogOpen, setIsAddDialogOpen] = useState(false);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [formData, setFormData] = useState<CreateStaticReservationRequest>({
    mac_address: "",
    ip_address: "",
    hostname: "",
  });

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      const newReservation = await createStaticReservation(networkId, {
        ...formData,
        hostname: formData.hostname || undefined,
      });
      onReservationsChange([...reservations, newReservation]);
      setIsAddDialogOpen(false);
      setFormData({ mac_address: "", ip_address: "", hostname: "" });
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create reservation");
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleDelete = async (id: number) => {
    setError(null);
    try {
      await deleteStaticReservation(id);
      onReservationsChange(reservations.filter((r) => r.id !== id));
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to delete reservation");
    }
  };

  const columns: ColumnDef<StaticReservation>[] = [
    {
      accessorKey: "mac_address",
      header: "MAC Address",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("mac_address")}</span>
      ),
    },
    {
      accessorKey: "ip_address",
      header: "IP Address",
      cell: ({ row }) => (
        <span className="font-mono text-xs">{row.getValue("ip_address")}</span>
      ),
    },
    {
      accessorKey: "hostname",
      header: "Hostname",
      cell: ({ row }) => {
        const hostname = row.getValue("hostname") as string | undefined;
        return hostname ? (
          <span className="text-sm">{hostname}</span>
        ) : (
          <span className="text-muted-foreground text-sm">—</span>
        );
      },
    },
    {
      id: "actions",
      header: "Actions",
      cell: ({ row }) => {
        return (
          <AlertDialog>
            <AlertDialogTrigger asChild>
              <Button variant="outline" size="sm" aria-label="Delete reservation">
                <Trash2 className="h-4 w-4" />
              </Button>
            </AlertDialogTrigger>
            <AlertDialogContent>
              <AlertDialogHeader>
                <AlertDialogTitle>Delete Static Reservation</AlertDialogTitle>
                <AlertDialogDescription>
                  Are you sure you want to delete the static reservation for{" "}
                  {row.original.mac_address}? This action cannot be undone.
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
        );
      },
    },
  ];

  const table = useReactTable({
    data: reservations,
    columns,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <div className="space-y-4">
      <div className="flex justify-end">
        <Dialog open={isAddDialogOpen} onOpenChange={setIsAddDialogOpen}>
          <DialogTrigger asChild>
            <Button size="sm">
              <Plus className="h-4 w-4 mr-2" />
              Add Reservation
            </Button>
          </DialogTrigger>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Add Static Reservation</DialogTitle>
              <DialogDescription>
                Reserve a specific IP address for a MAC address.
              </DialogDescription>
            </DialogHeader>
            <form onSubmit={handleAdd} className="space-y-4">
              {error && (
                <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md text-sm">
                  {error}
                </div>
              )}
              <div className="space-y-2">
                <Label htmlFor="mac-address">MAC Address *</Label>
                <Input
                  id="mac-address"
                  value={formData.mac_address}
                  onChange={(e) => setFormData({ ...formData, mac_address: e.target.value })}
                  placeholder="e.g., 00:11:22:33:44:55"
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="ip-address">IP Address *</Label>
                <Input
                  id="ip-address"
                  value={formData.ip_address}
                  onChange={(e) => setFormData({ ...formData, ip_address: e.target.value })}
                  placeholder="e.g., 192.168.1.10"
                  required
                />
              </div>
              <div className="space-y-2">
                <Label htmlFor="hostname">Hostname (Optional)</Label>
                <Input
                  id="hostname"
                  value={formData.hostname}
                  onChange={(e) => setFormData({ ...formData, hostname: e.target.value })}
                  placeholder="e.g., server01"
                />
              </div>
              <DialogFooter>
                <Button type="submit" disabled={isSubmitting}>
                  {isSubmitting ? "Adding..." : "Add Reservation"}
                </Button>
              </DialogFooter>
            </form>
          </DialogContent>
        </Dialog>
      </div>

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
                    No static reservations. Add a reservation to assign a fixed IP to a device.
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
