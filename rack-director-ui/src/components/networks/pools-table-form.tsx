import type { CreateDhcpPoolRequest } from "@/lib/client";
import { Label } from "../ui/label";
import { Input } from "../ui/input";
import { DialogFooter } from "../ui/dialog";
import { Button } from "../ui/button";

interface PoolsTableFormProps {
  formData: CreateDhcpPoolRequest;
  onSubmit: (e: React.FormEvent) => void;
  setFormData: (data: CreateDhcpPoolRequest) => void;
  isSubmitting: boolean;
  error: String | null;
  editingPool: boolean;
}

export default function PoolsTableForm({ formData, onSubmit, setFormData, isSubmitting, error, editingPool }: PoolsTableFormProps) {
  return (
    <form onSubmit={onSubmit} className="space-y-4">
      {error && (
        <div className="bg-destructive/10 border border-destructive text-destructive px-4 py-3 rounded-md text-sm">
          {error}
        </div>
      )}
      <div className="space-y-2">
        <Label htmlFor="pool-name">Pool Name *</Label>
        <Input
          id="pool-name"
          value={formData.name}
          onChange={(e) => setFormData({ ...formData, name: e.target.value })}
          placeholder="e.g., Main Pool"
          required
        />
      </div>
      <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
        <div className="space-y-2">
          <Label htmlFor="range-start">Range Start *</Label>
          <Input
            id="range-start"
            value={formData.range_start}
            onChange={(e) => setFormData({ ...formData, range_start: e.target.value })}
            placeholder="e.g., 192.168.1.100"
            required
          />
        </div>
        <div className="space-y-2">
          <Label htmlFor="range-end">Range End *</Label>
          <Input
            id="range-end"
            value={formData.range_end}
            onChange={(e) => setFormData({ ...formData, range_end: e.target.value })}
            placeholder="e.g., 192.168.1.200"
            required
          />
        </div>
      </div>
      <DialogFooter>
        <Button type="submit" disabled={isSubmitting}>
          {isSubmitting ? "Saving..." : editingPool ? "Update Pool" : "Add Pool"}
        </Button>
      </DialogFooter>
    </form>
  );
}
