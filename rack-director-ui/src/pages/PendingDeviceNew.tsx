import { useState, useEffect } from "react";
import { useNavigate } from "react-router";
import { Button } from "@/components/ui/button";
import { PageHeader } from "@/components/ui/page-header";
import { FormFieldError } from "@/components/ui/form-field-error";
import { useFieldErrors } from "@/hooks/useFieldErrors";
import { SeenMacInput } from "@/components/devices/seen-mac-input";
import {
  createPendingDevice,
  getNetworks,
  ValidationError,
  type DhcpNetwork,
} from "@/lib/client";

const selectClassName =
  "w-full bg-bg-base border border-border text-xs text-text-primary px-3 py-2 rounded-sm focus:outline-none focus:border-accent focus:shadow-[0_0_0_1px_var(--color-accent)]";

const labelClassName =
  "block text-xs font-semibold text-text-secondary uppercase tracking-[0.5px] mb-1";

export default function PendingDeviceNew() {
  const navigate = useNavigate();
  const { clearAllErrors, clearFieldError, setErrors, getError } = useFieldErrors();

  const [networkId, setNetworkId] = useState("");
  const [macAddress, setMacAddress] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [networks, setNetworks] = useState<DhcpNetwork[]>([]);

  useEffect(() => {
    getNetworks()
      .then((nets) => {
        setNetworks(nets);
      })
      .catch(() => {
        // Non-fatal: network select will remain empty
      });
  }, []);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    clearAllErrors();
    setIsSubmitting(true);

    try {
      await createPendingDevice({
        mac_address: macAddress,
        network_id: parseInt(networkId),
      });
      navigate("/devices");
    } catch (err) {
      if (err instanceof ValidationError) {
        setErrors(err.errors);
        setError("Please fix the validation errors below");
      } else {
        setError(err instanceof Error ? err.message : "Failed to create pending device");
      }
      setIsSubmitting(false);
    }
  };

  return (
    <div>
      <PageHeader
        breadcrumbs={[
          { label: "Dashboard", href: "/" },
          { label: "Devices", href: "/devices" },
          { label: "Add Pending Device" },
        ]}
        title="Add Pending Device"
        description="Pre-register a device by MAC address. It will be automatically discovered when it boots."
      />

      <form onSubmit={handleSubmit} style={{ maxWidth: 700 }}>
        {error && (
          <div className="mb-4 px-3 py-2 bg-error-bg border-l-[3px] border-status-broken text-xs text-status-broken">
            {error}
          </div>
        )}

        {/* Device Configuration card */}
        <div className="border border-border bg-bg-surface mb-4">
          <div className="px-3 py-2 border-b border-border">
            <span className="text-sm font-semibold text-text-primary">Device Configuration</span>
          </div>
          <div className="p-4 grid grid-cols-1 sm:grid-cols-2 gap-4">
            {/* Network */}
            <div>
              <label
                htmlFor="networkId"
                className={labelClassName}
              >
                Network <span className="text-status-broken">*</span>
              </label>
              <select
                id="networkId"
                value={networkId}
                onChange={(e) => { setNetworkId(e.target.value); clearFieldError("network_id"); }}
                required
                aria-invalid={!!getError("network_id")}
                className={selectClassName}
              >
                <option value="">Select a network...</option>
                {networks.map((net) => (
                  <option key={net.id} value={net.id}>
                    {net.name} ({net.subnet})
                  </option>
                ))}
              </select>
              <FormFieldError error={getError("network_id")} />
            </div>

            {/* MAC Address */}
            <div>
              <label
                htmlFor="macAddress"
                className={labelClassName}
              >
                MAC Address <span className="text-status-broken">*</span>
              </label>
              <SeenMacInput
                value={macAddress}
                onChange={(m) => { setMacAddress(m); clearFieldError("mac_address"); }}
                networkId={networkId ? parseInt(networkId) : null}
                error={getError("mac_address")}
              />
            </div>
          </div>
        </div>

        {/* Form actions */}
        <div className="flex gap-2">
          <Button type="submit" disabled={isSubmitting}>
            {isSubmitting ? "Adding..." : "Add Pending Device"}
          </Button>
          <Button
            type="button"
            variant="secondary"
            onClick={() => navigate("/devices")}
            disabled={isSubmitting}
          >
            Cancel
          </Button>
        </div>
      </form>
    </div>
  );
}
