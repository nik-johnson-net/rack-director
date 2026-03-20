import { useState } from "react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Trash2, Plus } from "lucide-react";
import {
  putDeviceLabelOverride,
  deleteDeviceLabelOverride,
  getDevice,
  type Device,
} from "@/lib/client";

interface DiskLabelOverridesProps {
  uuid: string;
  device: Device;
  onDeviceUpdate: (device: Device) => void;
  onError: (error: string) => void;
}

export function DiskLabelOverrides({
  uuid,
  device,
  onDeviceUpdate,
  onError,
}: DiskLabelOverridesProps) {
  const overrides = device.attributes?.disk_label_overrides ?? {};
  const overrideEntries = Object.entries(overrides);

  const [newLabel, setNewLabel] = useState("");
  const [newPath, setNewPath] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [removing, setRemoving] = useState<Set<string>>(new Set());

  const handleAdd = async (e: React.FormEvent) => {
    e.preventDefault();
    const label = newLabel.trim();
    const path = newPath.trim();
    if (!label || !path) return;

    setSubmitting(true);
    onError("");
    try {
      await putDeviceLabelOverride(uuid, { label, path });
      const updated = await getDevice(uuid);
      onDeviceUpdate(updated);
      setNewLabel("");
      setNewPath("");
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to set label override");
    } finally {
      setSubmitting(false);
    }
  };

  const handleRemove = async (label: string) => {
    setRemoving((prev) => new Set(prev).add(label));
    onError("");
    try {
      await deleteDeviceLabelOverride(uuid, label);
      const updated = await getDevice(uuid);
      onDeviceUpdate(updated);
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to remove label override");
    } finally {
      setRemoving((prev) => {
        const next = new Set(prev);
        next.delete(label);
        return next;
      });
    }
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle>Disk Label Overrides</CardTitle>
        <CardDescription>
          Override platform disk label assignments for this device
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {overrideEntries.length > 0 ? (
          <table className="min-w-full text-sm border rounded-md overflow-hidden">
            <thead>
              <tr className="border-b bg-muted/50">
                <th className="text-left py-2 px-3 font-medium">Label</th>
                <th className="text-left py-2 px-3 font-medium">Path</th>
                <th className="py-2 px-3 w-12"></th>
              </tr>
            </thead>
            <tbody>
              {overrideEntries.map(([label, path]) => (
                <tr key={label} className="border-b last:border-0">
                  <td className="py-2 px-3">
                    <Badge variant="secondary">{label}</Badge>
                  </td>
                  <td className="py-2 px-3 font-mono text-xs break-all">{path}</td>
                  <td className="py-2 px-3">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7 text-destructive hover:text-destructive hover:bg-destructive/10"
                      onClick={() => handleRemove(label)}
                      disabled={removing.has(label)}
                      aria-label={`Remove label override for ${label}`}
                    >
                      <Trash2 className="h-4 w-4" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        ) : (
          <p className="text-sm text-muted-foreground">No label overrides configured</p>
        )}

        <form onSubmit={handleAdd} className="space-y-3 pt-2 border-t">
          <p className="text-sm font-medium">Add Override</p>
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
            <div className="space-y-1">
              <Label htmlFor="override-label">Label</Label>
              <Input
                id="override-label"
                placeholder="e.g., ROOT"
                value={newLabel}
                onChange={(e) => setNewLabel(e.target.value)}
                disabled={submitting}
              />
            </div>
            <div className="space-y-1">
              <Label htmlFor="override-path">Path</Label>
              <Input
                id="override-path"
                placeholder="e.g., /dev/disk/by-path/..."
                value={newPath}
                onChange={(e) => setNewPath(e.target.value)}
                disabled={submitting}
              />
            </div>
          </div>
          <Button
            type="submit"
            disabled={submitting || !newLabel.trim() || !newPath.trim()}
            size="sm"
          >
            <Plus className="h-4 w-4 mr-1" />
            {submitting ? "Saving..." : "Add Override"}
          </Button>
        </form>
      </CardContent>
    </Card>
  );
}
