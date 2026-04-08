import { useState, useEffect } from "react";
import {
  AlertDialog,
  AlertDialogContent,
  AlertDialogHeader,
  AlertDialogTitle,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogCancel,
  AlertDialogAction,
} from "@/components/ui/alert-dialog";
import { Label } from "@/components/ui/label";
import type { Role } from "@/lib/client";

interface ProvisionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  availableRoles: Role[];
  currentRoleId?: number | null;
  onConfirm: (roleId: number) => Promise<void>;
}

export function ProvisionDialog({
  open,
  onOpenChange,
  availableRoles,
  currentRoleId,
  onConfirm,
}: ProvisionDialogProps) {
  const [selectedRoleId, setSelectedRoleId] = useState<number | "">("");
  const [isProvisioning, setIsProvisioning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setSelectedRoleId(currentRoleId ?? "");
      setError(null);
    }
  }, [open, currentRoleId]);

  const handleConfirm = async () => {
    if (!selectedRoleId) {
      setError("Please select a role before provisioning.");
      return;
    }
    setIsProvisioning(true);
    setError(null);
    try {
      await onConfirm(selectedRoleId as number);
      onOpenChange(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to provision device");
    } finally {
      setIsProvisioning(false);
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Provision Device</AlertDialogTitle>
          <AlertDialogDescription>
            Select a role for this device. The role determines the OS, disk layout, and
            configuration that will be applied during provisioning.
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="py-2 space-y-3">
          <div className="space-y-1">
            <Label htmlFor="provision-role-select" className="text-xs uppercase text-text-secondary tracking-wide">
              Role
            </Label>
            <select
              id="provision-role-select"
              value={selectedRoleId}
              onChange={(e) => {
                setSelectedRoleId(e.target.value ? parseInt(e.target.value) : "");
                setError(null);
              }}
              className="w-full bg-bg-base border border-border text-text-primary text-sm px-3 py-2 focus:outline-none focus:border-accent"
              disabled={isProvisioning}
            >
              <option value="">Select a role...</option>
              {availableRoles.map((role) => (
                <option key={role.id} value={role.id}>
                  {role.name} — {role.os_name} {role.os_release}
                </option>
              ))}
            </select>
          </div>

          {error && (
            <p className="text-xs text-status-broken">{error}</p>
          )}
        </div>

        <AlertDialogFooter>
          <AlertDialogCancel disabled={isProvisioning}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={isProvisioning || !selectedRoleId}
          >
            {isProvisioning ? "Provisioning..." : "Provision"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
