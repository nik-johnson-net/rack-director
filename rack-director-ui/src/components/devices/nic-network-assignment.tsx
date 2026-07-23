import { useState } from "react";
import { Save, X } from "lucide-react";
import {
  setNicAssignment,
  clearNicAssignment,
  type NetworkInterface,
  type DhcpNetwork,
} from "@/lib/client";

interface NicNetworkAssignmentProps {
  uuid: string;
  nic: NetworkInterface;
  networks: DhcpNetwork[];
  onNicUpdate: (updated: NetworkInterface) => void;
  onError: (error: string) => void;
}

export function NicNetworkAssignment({
  uuid,
  nic,
  networks,
  onNicUpdate,
  onError,
}: NicNetworkAssignmentProps) {
  // Effective values: manual assignment takes precedence over DHCP-derived
  const effectiveNetworkId = nic.assigned_network_id ?? nic.network_id ?? null;
  const effectiveIp = nic.assigned_ip ?? nic.ip_address ?? null;

  const [networkId, setNetworkId] = useState<number | null>(effectiveNetworkId);
  const [ip, setIp] = useState<string>(effectiveIp ?? "");
  const [saving, setSaving] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [inlineError, setInlineError] = useState<string | null>(null);

  const hasManualAssignment =
    nic.assigned_network_id != null || nic.assigned_ip != null;

  const isDirty =
    networkId !== effectiveNetworkId ||
    ip !== (effectiveIp ?? "");

  const handleSave = async () => {
    setInlineError(null);
    onError("");
    setSaving(true);
    try {
      await setNicAssignment(uuid, {
        mac_address: nic.mac_address,
        network_id: networkId,
        ip: ip.trim() || null,
      });
      onNicUpdate({
        ...nic,
        assigned_network_id: networkId,
        assigned_ip: ip.trim() || null,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to save NIC assignment";
      setInlineError(msg);
    } finally {
      setSaving(false);
    }
  };

  const handleClear = async () => {
    setInlineError(null);
    onError("");
    setClearing(true);
    try {
      await clearNicAssignment(uuid, nic.mac_address);
      setNetworkId(nic.network_id ?? null);
      setIp(nic.ip_address ?? "");
      onNicUpdate({
        ...nic,
        assigned_network_id: null,
        assigned_ip: null,
      });
    } catch (err) {
      const msg = err instanceof Error ? err.message : "Failed to clear NIC assignment";
      setInlineError(msg);
    } finally {
      setClearing(false);
    }
  };

  const inputClass =
    "bg-bg-base border border-border text-xs text-text-primary px-2 py-1 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)] placeholder:text-text-muted";

  return (
    <div className="space-y-1.5">
      {/* Network dropdown */}
      <div className="flex items-center gap-2">
        <select
          value={networkId ?? ""}
          onChange={(e) => {
            setNetworkId(e.target.value ? Number(e.target.value) : null);
            setInlineError(null);
          }}
          disabled={saving || clearing}
          aria-label="Assigned network"
          className={`${inputClass} w-40`}
          style={{
            backgroundImage: `url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 24 24' fill='none' stroke='%236b7280' stroke-width='2'%3E%3Cpath d='M6 9l6 6 6-6'/%3E%3C/svg%3E")`,
            backgroundRepeat: "no-repeat",
            backgroundPosition: "right 0.5rem center",
            paddingRight: "1.75rem",
            appearance: "none",
          }}
        >
          <option value="">— none —</option>
          {networks.map((n) => (
            <option key={n.id} value={n.id}>
              {n.name}
            </option>
          ))}
        </select>

        {/* IP input */}
        <input
          type="text"
          value={ip}
          onChange={(e) => {
            setIp(e.target.value);
            setInlineError(null);
          }}
          placeholder="IP address"
          disabled={saving || clearing}
          aria-label="Assigned IP address"
          className={`${inputClass} w-32`}
        />

        {/* Save button — only shown when dirty */}
        {isDirty && (
          <button
            onClick={handleSave}
            disabled={saving || clearing}
            aria-label="Save NIC assignment"
            title="Save assignment"
            className="text-accent hover:text-accent-hover disabled:opacity-40 transition-colors cursor-pointer"
          >
            <Save className="size-3.5" />
          </button>
        )}

        {/* Clear button — only shown when a manual assignment exists */}
        {hasManualAssignment && !isDirty && (
          <button
            onClick={handleClear}
            disabled={saving || clearing}
            aria-label="Clear NIC assignment"
            title="Clear manual assignment"
            className="text-text-muted hover:text-status-broken disabled:opacity-40 transition-colors cursor-pointer"
          >
            <X className="size-3.5" />
          </button>
        )}
      </div>

      {/* Inline validation/API error */}
      {inlineError && (
        <p className="text-xs text-status-broken">{inlineError}</p>
      )}

      {/* Show effective vs manual indicator */}
      {hasManualAssignment && !isDirty && (
        <p className="text-xs text-accent">manual</p>
      )}
      {!hasManualAssignment && effectiveNetworkId && !isDirty && (
        <p className="text-xs text-text-muted">via dhcp</p>
      )}
    </div>
  );
}
