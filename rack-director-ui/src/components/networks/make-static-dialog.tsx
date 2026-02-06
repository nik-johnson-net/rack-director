import { useState, useEffect } from "react";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { FormField } from "@/components/ui/form-field";
import type { DhcpLease } from "@/lib/client";

interface MakeStaticDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  lease: DhcpLease | null;
  subnet?: string;
  onConfirm: (ip: string, hostname?: string) => Promise<void>;
}

export function MakeStaticDialog({ open, onOpenChange, lease, subnet, onConfirm }: MakeStaticDialogProps) {
  const [ip, setIp] = useState("");
  const [hostname, setHostname] = useState("");
  const [isLoading, setIsLoading] = useState(false);

  // Reset form when dialog opens
  useEffect(() => {
    if (open && lease) {
      setIp(lease.ip_address);
      setHostname(lease.hostname ?? "");
    }
  }, [open, lease]);

  const handleConfirm = async () => {
    setIsLoading(true);
    try {
      await onConfirm(ip, hostname || undefined);
      onOpenChange(false);
    } finally {
      setIsLoading(false);
    }
  };

  const handleUseCurrentIp = () => {
    if (lease) {
      setIp(lease.ip_address);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Make IP Address Static</DialogTitle>
          <DialogDescription>
            Assign a static IP address to this MAC address. You can use the current lease IP or specify a custom one.
          </DialogDescription>
        </DialogHeader>

        {lease && (
          <div className="space-y-4">
            <div className="bg-muted p-3 rounded-md">
              <div className="text-sm">
                <span className="font-medium">MAC Address: </span>
                <span className="font-mono">{lease.mac_address}</span>
              </div>
              <div className="text-sm mt-1">
                <span className="font-medium">Current IP: </span>
                <span className="font-mono">{lease.ip_address}</span>
              </div>
            </div>

            <div className="space-y-2">
              <div className="flex gap-2">
                <div className="flex-1">
                  <FormField
                    id="static-ip"
                    label="IP Address"
                    required
                    value={ip}
                    onChange={setIp}
                    placeholder="e.g., 192.168.1.100"
                  />
                </div>
                <div className="pt-8">
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={handleUseCurrentIp}
                  >
                    Reset
                  </Button>
                </div>
              </div>
              {subnet && (
                <p className="text-xs text-muted-foreground">
                  Subnet: {subnet}
                </p>
              )}
            </div>

            <FormField
              id="static-hostname"
              label="Hostname"
              value={hostname}
              onChange={setHostname}
              placeholder="e.g., server-01"
              helperText="Optional hostname for this reservation"
            />
          </div>
        )}

        <DialogFooter>
          <Button
            type="button"
            variant="outline"
            onClick={() => onOpenChange(false)}
            disabled={isLoading}
          >
            Cancel
          </Button>
          <Button
            type="button"
            onClick={handleConfirm}
            disabled={isLoading}
          >
            {isLoading ? "Creating..." : "Create Static Reservation"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
