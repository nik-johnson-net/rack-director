import { useState, useEffect } from "react";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import type { DhcpPool, CreateDhcpPoolRequest } from "@/lib/client";
import PoolsTableForm from "./pools-table-form";

interface PoolDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  pool?: DhcpPool | null;
  onSave: (pool: CreateDhcpPoolRequest) => Promise<void>;
}

export function PoolDialog({ open, onOpenChange, pool, onSave }: PoolDialogProps) {
  const [formData, setFormData] = useState<CreateDhcpPoolRequest>({
    name: "",
    range_start: "",
    range_end: "",
  });
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset form when dialog opens or pool changes
  useEffect(() => {
    if (open) {
      if (pool) {
        setFormData({
          name: pool.name,
          range_start: pool.range_start,
          range_end: pool.range_end,
        });
      } else {
        setFormData({
          name: "",
          range_start: "",
          range_end: "",
        });
      }
      setError(null);
    }
  }, [open, pool]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    setIsSubmitting(true);

    try {
      await onSave(formData);
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to ${pool ? "update" : "create"} pool`);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>{pool ? "Edit Pool" : "Add Pool"}</DialogTitle>
          <DialogDescription>
            {pool ? "Update the pool configuration." : "Create a new IP address pool for this network."}
          </DialogDescription>
        </DialogHeader>
        <PoolsTableForm
          onSubmit={handleSubmit}
          setFormData={setFormData}
          editingPool={!!pool}
          formData={formData}
          isSubmitting={isSubmitting}
          error={error}
        />
      </DialogContent>
    </Dialog>
  );
}
