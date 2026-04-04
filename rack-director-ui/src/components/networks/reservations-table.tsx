import { useState } from "react";
import type { StaticReservation, CreateStaticReservationRequest } from "@/lib/client";
import { createStaticReservation, deleteStaticReservation } from "@/lib/client";
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

const inputCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

const labelCls =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";

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

  return (
    <div className="space-y-3">
      {error && (
        <div className="px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
          {error}
        </div>
      )}

      <div className="flex justify-end">
        <button
          type="button"
          onClick={() => setIsAddDialogOpen(true)}
          className="px-3 py-1 h-7 text-xs font-medium bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover transition-colors cursor-pointer"
        >
          + Add Reservation
        </button>
      </div>

      {/* Add Reservation Dialog */}
      {isAddDialogOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center"
          role="dialog"
          aria-modal="true"
          aria-label="Add Static Reservation"
        >
          <div
            className="absolute inset-0 bg-black/60"
            onClick={() => setIsAddDialogOpen(false)}
          />
          <div className="relative z-10 w-full max-w-md bg-bg-overlay border border-border shadow-xl">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <div>
                <h2 className="text-sm font-semibold text-text-primary">Add Static Reservation</h2>
                <p className="text-xs text-text-secondary mt-0.5">
                  Reserve a specific IP address for a MAC address.
                </p>
              </div>
              <button
                type="button"
                onClick={() => setIsAddDialogOpen(false)}
                className="text-text-muted hover:text-text-primary transition-colors cursor-pointer text-lg leading-none"
                aria-label="Close dialog"
              >
                ×
              </button>
            </div>

            <form onSubmit={handleAdd} className="p-4 space-y-4">
              {error && (
                <div className="px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
                  {error}
                </div>
              )}

              <div>
                <label htmlFor="res-mac" className={labelCls}>
                  MAC Address <span className="text-status-broken">*</span>
                </label>
                <input
                  id="res-mac"
                  type="text"
                  value={formData.mac_address}
                  onChange={(e) => setFormData({ ...formData, mac_address: e.target.value })}
                  placeholder="e.g., 00:11:22:33:44:55"
                  required
                  className={inputCls}
                />
              </div>

              <div>
                <label htmlFor="res-ip" className={labelCls}>
                  IP Address <span className="text-status-broken">*</span>
                </label>
                <input
                  id="res-ip"
                  type="text"
                  value={formData.ip_address}
                  onChange={(e) => setFormData({ ...formData, ip_address: e.target.value })}
                  placeholder="e.g., 192.168.1.10"
                  required
                  className={inputCls}
                />
              </div>

              <div>
                <label htmlFor="res-hostname" className={labelCls}>
                  Hostname (Optional)
                </label>
                <input
                  id="res-hostname"
                  type="text"
                  value={formData.hostname}
                  onChange={(e) => setFormData({ ...formData, hostname: e.target.value })}
                  placeholder="e.g., server01"
                  className={inputCls}
                />
              </div>

              <div className="flex justify-end gap-2 pt-2 border-t border-border">
                <button
                  type="button"
                  onClick={() => setIsAddDialogOpen(false)}
                  disabled={isSubmitting}
                  className="px-4 py-2 h-8 text-xs font-medium bg-bg-surface text-text-primary border border-border rounded hover:bg-bg-raised disabled:opacity-50 disabled:pointer-events-none transition-colors cursor-pointer"
                >
                  Cancel
                </button>
                <button
                  type="submit"
                  disabled={isSubmitting}
                  className="px-4 py-2 h-8 text-xs font-medium bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover disabled:opacity-50 disabled:pointer-events-none transition-colors cursor-pointer"
                >
                  {isSubmitting ? "Adding..." : "Add Reservation"}
                </button>
              </div>
            </form>
          </div>
        </div>
      )}

      <div className="border border-border">
        <table className="w-full border-collapse">
          <thead>
            <tr className="bg-bg-raised">
              {(["MAC Address", "IP Address", "Hostname", ""] as const).map((col, i) => (
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
            {reservations.length === 0 ? (
              <tr>
                <td colSpan={4} className="px-3 py-6 text-center text-xs text-text-muted">
                  No static reservations. Add a reservation to assign a fixed IP to a device.
                </td>
              </tr>
            ) : (
              reservations.map((res, idx) => {
                const rowBg = idx % 2 === 0 ? "bg-bg-surface" : "bg-bg-base";
                return (
                  <tr
                    key={res.id}
                    className={`${rowBg} hover:bg-bg-raised border-b border-border-muted last:border-b-0 transition-colors`}
                  >
                    <td className="px-3 py-2 text-xs font-mono text-text-primary">
                      {res.mac_address}
                    </td>
                    <td className="px-3 py-2 text-xs font-mono text-text-secondary">
                      {res.ip_address}
                    </td>
                    <td className="px-3 py-2 text-xs text-text-secondary">
                      {res.hostname || <span className="text-text-muted">—</span>}
                    </td>
                    <td className="px-3 py-2">
                      <AlertDialog>
                        <AlertDialogTrigger asChild>
                          <button
                            type="button"
                            className="text-xs text-text-muted hover:text-status-broken transition-colors cursor-pointer"
                            aria-label={`Delete reservation for ${res.mac_address}`}
                          >
                            delete
                          </button>
                        </AlertDialogTrigger>
                        <AlertDialogContent>
                          <AlertDialogHeader>
                            <AlertDialogTitle>Delete Static Reservation</AlertDialogTitle>
                            <AlertDialogDescription>
                              Are you sure you want to delete the static reservation for{" "}
                              {res.mac_address}? This action cannot be undone.
                            </AlertDialogDescription>
                          </AlertDialogHeader>
                          <AlertDialogFooter>
                            <AlertDialogCancel>Cancel</AlertDialogCancel>
                            <AlertDialogAction onClick={() => handleDelete(res.id)}>
                              Delete
                            </AlertDialogAction>
                          </AlertDialogFooter>
                        </AlertDialogContent>
                      </AlertDialog>
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
