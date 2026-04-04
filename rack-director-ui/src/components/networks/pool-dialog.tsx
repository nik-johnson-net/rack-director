import { useState, useEffect } from "react";
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

  useEffect(() => {
    if (open) {
      if (pool) {
        setFormData({
          name: pool.name,
          range_start: pool.range_start,
          range_end: pool.range_end,
        });
      } else {
        setFormData({ name: "", range_start: "", range_end: "" });
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

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      role="dialog"
      aria-modal="true"
      aria-label={pool ? "Edit Pool" : "Add Pool"}
    >
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/60"
        onClick={() => onOpenChange(false)}
      />

      {/* Dialog panel */}
      <div className="relative z-10 w-full max-w-md bg-bg-overlay border border-border shadow-xl">
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <div>
            <h2 className="text-sm font-semibold text-text-primary">
              {pool ? "Edit Pool" : "Add Pool"}
            </h2>
            <p className="text-xs text-text-secondary mt-0.5">
              {pool
                ? "Update the pool configuration."
                : "Create a new IP address pool for this network."}
            </p>
          </div>
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            className="text-text-muted hover:text-text-primary transition-colors cursor-pointer text-lg leading-none"
            aria-label="Close dialog"
          >
            ×
          </button>
        </div>

        {/* Body */}
        <div className="p-4">
          <PoolsTableForm
            onSubmit={handleSubmit}
            setFormData={setFormData}
            editingPool={!!pool}
            formData={formData}
            isSubmitting={isSubmitting}
            error={error}
          />
        </div>
      </div>
    </div>
  );
}
