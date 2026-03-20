import { useState, useEffect } from "react";
import { Button } from "@/components/ui/button";
import { AlertCircle, X } from "lucide-react";
import {
  getDeviceWarnings,
  dismissDeviceWarning,
  type DeviceWarning,
} from "@/lib/client";

interface DeviceWarningsProps {
  uuid: string;
  onError: (error: string) => void;
}

export function DeviceWarnings({ uuid, onError }: DeviceWarningsProps) {
  const [warnings, setWarnings] = useState<DeviceWarning[]>([]);
  const [loading, setLoading] = useState(true);
  const [dismissing, setDismissing] = useState<Set<number>>(new Set());

  useEffect(() => {
    const fetchWarnings = async () => {
      try {
        const data = await getDeviceWarnings(uuid);
        setWarnings(data);
      } catch (err) {
        onError(err instanceof Error ? err.message : "Failed to load warnings");
      } finally {
        setLoading(false);
      }
    };
    fetchWarnings();
  }, [uuid, onError]);

  const handleDismiss = async (warningId: number) => {
    setDismissing((prev) => new Set(prev).add(warningId));
    try {
      await dismissDeviceWarning(uuid, warningId);
      setWarnings((prev) => prev.filter((w) => w.id !== warningId));
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to dismiss warning");
    } finally {
      setDismissing((prev) => {
        const next = new Set(prev);
        next.delete(warningId);
        return next;
      });
    }
  };

  if (loading || warnings.length === 0) {
    return null;
  }

  return (
    <div className="space-y-2">
      {warnings.map((warning) => (
        <div
          key={warning.id}
          className="flex items-start gap-3 rounded-md border border-yellow-300 bg-yellow-50 px-4 py-3 text-yellow-900"
          role="alert"
        >
          <AlertCircle className="h-5 w-5 mt-0.5 shrink-0 text-yellow-600" aria-hidden="true" />
          <div className="flex-1 min-w-0">
            <p className="font-semibold text-sm">{warning.code}</p>
            <p className="text-sm mt-0.5">{warning.message}</p>
          </div>
          <Button
            variant="ghost"
            size="icon"
            className="h-6 w-6 shrink-0 text-yellow-700 hover:text-yellow-900 hover:bg-yellow-100"
            onClick={() => handleDismiss(warning.id)}
            disabled={dismissing.has(warning.id)}
            aria-label={`Dismiss warning: ${warning.code}`}
          >
            <X className="h-4 w-4" />
          </Button>
        </div>
      ))}
    </div>
  );
}
