import { useState, useEffect } from "react";
import type { DhcpLease } from "@/lib/client";

interface MakeStaticDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  lease: DhcpLease | null;
  subnet?: string;
  onConfirm: (ip: string, hostname?: string) => Promise<void>;
}

const inputCls =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

const labelCls =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";

export function MakeStaticDialog({
  open,
  onOpenChange,
  lease,
  subnet,
  onConfirm,
}: MakeStaticDialogProps) {
  const [ip, setIp] = useState("");
  const [hostname, setHostname] = useState("");
  const [isLoading, setIsLoading] = useState(false);

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
    if (lease) setIp(lease.ip_address);
  };

  if (!open) return null;

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center"
      role="dialog"
      aria-modal="true"
      aria-label="Make IP Address Static"
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
            <h2 className="text-sm font-semibold text-text-primary">Make IP Address Static</h2>
            <p className="text-xs text-text-secondary mt-0.5">
              Assign a static IP address to this MAC address.
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
        {lease && (
          <div className="p-4 space-y-4">
            {/* Info block */}
            <div className="bg-bg-raised border border-border px-3 py-2 grid grid-cols-[140px_1fr] gap-y-1">
              <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">MAC Address</span>
              <span className="text-xs font-mono text-text-primary">{lease.mac_address}</span>
              <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">Current IP</span>
              <span className="text-xs font-mono text-text-primary">{lease.ip_address}</span>
              {subnet && (
                <>
                  <span className="text-xs text-text-secondary uppercase tracking-[0.5px]">Subnet</span>
                  <span className="text-xs font-mono text-text-primary">{subnet}</span>
                </>
              )}
            </div>

            {/* IP Address field */}
            <div>
              <label htmlFor="static-ip" className={labelCls}>
                IP Address <span className="text-status-broken">*</span>
              </label>
              <div className="flex gap-2">
                <input
                  id="static-ip"
                  type="text"
                  value={ip}
                  onChange={(e) => setIp(e.target.value)}
                  placeholder="e.g., 192.168.1.100"
                  required
                  className={`${inputCls} flex-1`}
                />
                <button
                  type="button"
                  onClick={handleUseCurrentIp}
                  className="px-3 py-2 h-8 text-xs font-medium bg-bg-surface text-text-primary border border-border rounded-sm hover:bg-bg-raised transition-colors cursor-pointer whitespace-nowrap"
                >
                  Reset
                </button>
              </div>
            </div>

            {/* Hostname field */}
            <div>
              <label htmlFor="static-hostname" className={labelCls}>
                Hostname
              </label>
              <input
                id="static-hostname"
                type="text"
                value={hostname}
                onChange={(e) => setHostname(e.target.value)}
                placeholder="e.g., server-01"
                className={inputCls}
              />
              <p className="text-xs text-text-muted mt-1">Optional hostname for this reservation</p>
            </div>
          </div>
        )}

        {/* Footer */}
        <div className="flex justify-end gap-2 px-4 py-3 border-t border-border">
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            disabled={isLoading}
            className="px-4 py-2 h-8 text-xs font-medium bg-bg-surface text-text-primary border border-border rounded hover:bg-bg-raised disabled:opacity-50 disabled:pointer-events-none transition-colors cursor-pointer"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={handleConfirm}
            disabled={isLoading}
            className="px-4 py-2 h-8 text-xs font-medium bg-accent text-bg-base border border-accent rounded hover:bg-accent-hover disabled:opacity-50 disabled:pointer-events-none transition-colors cursor-pointer"
          >
            {isLoading ? "Creating..." : "Create Static Reservation"}
          </button>
        </div>
      </div>
    </div>
  );
}
