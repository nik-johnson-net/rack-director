/**
 * SeenMacInput — a controlled MAC address combobox that proactively shows
 * unregistered MAC addresses seen on the selected network (sourced from DHCP
 * leases), so operators can pick one instead of typing it from memory.
 *
 * Fetches its own data (leases + pending devices) on mount, non-fatally.
 * Filters by networkId, freshness (active/offered), and text prefix.
 * Shows all candidates immediately on focus — even when the field is empty.
 */

import { useState, useEffect, useMemo } from "react";
import { FormFieldError } from "@/components/ui/form-field-error";
import {
  getDhcpLeases,
  getPendingDevices,
  type DhcpLease,
  type PendingDevice,
} from "@/lib/client";

const inputClassName =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

interface SeenMacInputProps {
  value: string;
  onChange: (mac: string) => void;
  networkId: number | null;
  error?: string;
  id?: string;
}

export function SeenMacInput({
  value,
  onChange,
  networkId,
  error,
  id = "macAddress",
}: SeenMacInputProps) {
  const [showSuggestions, setShowSuggestions] = useState(false);
  const [leases, setLeases] = useState<DhcpLease[]>([]);
  const [existingPending, setExistingPending] = useState<PendingDevice[]>([]);

  useEffect(() => {
    Promise.all([getDhcpLeases(), getPendingDevices()])
      .then(([ls, pending]) => {
        setLeases(ls);
        setExistingPending(pending);
      })
      .catch(() => {
        // Non-fatal: manual entry still works without suggestions
      });
  }, []);

  const pendingMacs = useMemo(
    () => new Set(existingPending.map((p) => p.mac_address.toLowerCase())),
    [existingPending]
  );

  const candidates = useMemo(() => {
    const query = value.toLowerCase();

    return leases
      .filter((lease) => {
        // Skip leases already tied to a device
        if (lease.device_uuid) return false;
        // Skip MACs already registered as pending devices
        if (pendingMacs.has(lease.mac_address.toLowerCase())) return false;
        // Filter by selected network when one is chosen
        if (networkId !== null && lease.network_id !== networkId) return false;
        // Only keep fresh leases; treat missing state as included
        if (
          lease.state !== undefined &&
          lease.state !== "active" &&
          lease.state !== "offered"
        ) {
          return false;
        }
        // Text filter: if the user has typed something, require prefix match
        if (query && !lease.mac_address.toLowerCase().startsWith(query)) {
          return false;
        }
        return true;
      })
      .sort((a, b) => {
        // Sort most-recently-seen first using lease_start, falling back to lease_end
        const aTime =
          a.lease_start
            ? new Date(a.lease_start).getTime()
            : a.lease_end
            ? new Date(a.lease_end).getTime()
            : 0;
        const bTime =
          b.lease_start
            ? new Date(b.lease_start).getTime()
            : b.lease_end
            ? new Date(b.lease_end).getTime()
            : 0;
        return bTime - aTime;
      })
      .slice(0, 10);
  }, [value, networkId, leases, pendingMacs]);

  return (
    <div className="relative">
      <input
        id={id}
        type="text"
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
        }}
        onFocus={() => setShowSuggestions(true)}
        onBlur={() => setTimeout(() => setShowSuggestions(false), 150)}
        placeholder="e.g., aa:bb:cc:dd:ee:ff"
        required
        aria-invalid={!!error}
        autoComplete="off"
        className={inputClassName}
      />

      {showSuggestions && (
        <div className="absolute z-10 w-full bg-bg-surface border border-border mt-1 max-h-48 overflow-y-auto">
          {candidates.length > 0 ? (
            candidates.map((lease) => (
              <div
                key={lease.id}
                className="px-3 py-2 text-xs text-text-primary hover:bg-bg-raised cursor-pointer"
                onMouseDown={() => {
                  onChange(lease.mac_address);
                  setShowSuggestions(false);
                }}
              >
                {lease.mac_address} &mdash; {lease.ip_address}
              </div>
            ))
          ) : (
            <div className="px-3 py-2 text-xs text-text-muted">
              {networkId === null
                ? "Select a network to see MACs seen on it"
                : "No unregistered MACs seen on this network yet"}
            </div>
          )}
        </div>
      )}

      <FormFieldError error={error} />
    </div>
  );
}
