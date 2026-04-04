import { useState } from "react";
import { useNavigate } from "react-router";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import {
  getDevicePlatform,
  getDevice,
  assignDevicePlatform,
  type Platform,
  type Device,
} from "@/lib/client";

interface PlatformAssignmentProps {
  uuid: string;
  device: Device;
  assignedPlatform: Platform | null;
  availablePlatforms: Platform[];
  onPlatformUpdate: (platform: Platform | null, device: Device) => void;
  onError: (error: string) => void;
}

export function PlatformAssignment({
  uuid,
  device,
  assignedPlatform,
  availablePlatforms,
  onPlatformUpdate,
  onError,
}: PlatformAssignmentProps) {
  const navigate = useNavigate();
  const [selectedPlatformId, setSelectedPlatformId] = useState<number | null>(
    device.platform_id || null
  );
  const [assigningPlatform, setAssigningPlatform] = useState(false);

  const handleAssignPlatform = async () => {
    if (!uuid || !selectedPlatformId) return;

    setAssigningPlatform(true);
    onError("");

    try {
      await assignDevicePlatform(uuid, selectedPlatformId);
      const updatedPlatform = await getDevicePlatform(uuid);

      // Refresh device data
      const updatedDevice = await getDevice(uuid);
      onPlatformUpdate(updatedPlatform, updatedDevice);
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to assign platform");
    } finally {
      setAssigningPlatform(false);
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Platform Assignment</CardTitle>
        <CardDescription>
          Assign a hardware platform to this device
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {assignedPlatform ? (
          <div className="p-3 bg-bg-raised rounded border border-border">
            <div className="font-medium text-text-primary">
              <button
                onClick={() => navigate(`/platforms/${assignedPlatform.id}`)}
                className="text-accent hover:underline"
              >
                {assignedPlatform.name}
              </button>
            </div>
            <div className="text-xs text-text-secondary mt-1">
              {assignedPlatform.description || "No description"}
            </div>
            <div className="text-xs text-text-muted mt-2">
              {assignedPlatform.attributes.cpus.length > 0 && (
                <div>
                  {assignedPlatform.attributes.cpus.length}x {assignedPlatform.attributes.cpus[0].cores}-core {assignedPlatform.attributes.cpus[0].brand}
                </div>
              )}
              <div>
                {assignedPlatform.attributes.memory_gib}GB RAM, {assignedPlatform.attributes.disks.length} disk{assignedPlatform.attributes.disks.length !== 1 ? 's' : ''}, {assignedPlatform.attributes.nics.length} NIC{assignedPlatform.attributes.nics.length !== 1 ? 's' : ''}
              </div>
            </div>
          </div>
        ) : (
          <div className="text-sm text-muted-foreground">
            No platform assigned
          </div>
        )}

        <div className="space-y-2">
          <Label htmlFor="platform">Select Platform</Label>
          <select
            id="platform"
            value={selectedPlatformId || ''}
            onChange={(e) => setSelectedPlatformId(e.target.value ? parseInt(e.target.value) : null)}
            className="w-full bg-bg-raised border border-border rounded text-text-primary px-3 py-2 text-sm focus:outline-none focus:ring-1 focus:ring-accent"
          >
            <option value="">No platform</option>
            {availablePlatforms.map((platform) => (
              <option key={platform.id} value={platform.id}>
                {platform.name}
              </option>
            ))}
          </select>
        </div>

        <Button
          onClick={handleAssignPlatform}
          disabled={assigningPlatform || selectedPlatformId === device.platform_id}
          className="w-full"
        >
          {assigningPlatform ? "Assigning..." : "Assign Platform"}
        </Button>
      </CardContent>
    </Card>
  );
}
