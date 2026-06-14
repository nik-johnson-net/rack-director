import { useState, useEffect } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Power, RotateCcw, Zap } from "lucide-react";
import {
  type PowerState,
  type PowerAction,
  getDevicePower,
  setDevicePower,
} from "@/lib/client";

// ── Power confirm dialog ─────────────────────────────────────────────────────

type PowerConfirmDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  action: "off" | "cycle";
  onConfirm: () => Promise<void>;
};

function PowerConfirmDialog({ open, onOpenChange, action, onConfirm }: PowerConfirmDialogProps) {
  const [isSubmitting, setIsSubmitting] = useState(false);

  const handleConfirm = async () => {
    setIsSubmitting(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } finally {
      setIsSubmitting(false);
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {action === "off" ? "Power Off Device?" : "Power Cycle Device?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {action === "off"
              ? "This will send a hard power-off command to the BMC. The device will lose power immediately."
              : "This will send a power-cycle command to the BMC. The device will reboot immediately."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isSubmitting}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={isSubmitting}
            className="bg-destructive hover:bg-destructive/90 text-destructive-foreground"
          >
            {isSubmitting
              ? "Sending..."
              : action === "off"
              ? "Power Off"
              : "Power Cycle"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

// ── Power state badge ────────────────────────────────────────────────────────

function PowerStateBadge({ state, loading }: { state: PowerState | null; loading: boolean }) {
  if (loading) {
    return (
      <Badge variant="outline" className="text-text-muted">
        Checking...
      </Badge>
    );
  }
  if (state === "on") {
    return (
      <Badge variant="status-provisioned">
        <Power className="size-3" />
        On
      </Badge>
    );
  }
  if (state === "off") {
    return (
      <Badge variant="secondary">
        <Power className="size-3" />
        Off
      </Badge>
    );
  }
  // unknown or null
  return (
    <Badge variant="outline" className="text-text-muted">
      <Power className="size-3" />
      Unknown
    </Badge>
  );
}

// ── PowerControls ─────────────────────────────────────────────────────────────

export interface PowerControlsProps {
  /** Device UUID used for API calls. */
  uuid: string;
  /** Whether a BMC is currently configured on the device. When false the component renders nothing. */
  hasBmc: boolean;
  /** Called when a power action fails, with a human-readable error message. */
  onError: (error: string) => void;
}

/**
 * Renders the "Power State:" info row and "Power Controls" button group for a
 * device that has a BMC configured. Encapsulates the lazy power-state fetch,
 * all action handlers, and the confirmation dialog for destructive actions
 * (power-off and power-cycle).
 *
 * Intended to be embedded inside the BmcConfiguration discovered-BMC info block.
 */
export function PowerControls({ uuid, hasBmc, onError }: PowerControlsProps) {
  const [powerState, setPowerState] = useState<PowerState | null>(null);
  const [powerDriver, setPowerDriver] = useState<string | null>(null);
  const [powerLoading, setPowerLoading] = useState(false);
  const [powerActionInFlight, setPowerActionInFlight] = useState(false);

  // Confirmation dialog state
  const [confirmDialogOpen, setConfirmDialogOpen] = useState(false);
  const [pendingAction, setPendingAction] = useState<"off" | "cycle" | null>(null);

  // Fetch power state lazily when a BMC is present — never blocks the card render
  useEffect(() => {
    if (!hasBmc) return;
    let cancelled = false;

    setPowerLoading(true);
    getDevicePower(uuid).then((status) => {
      if (cancelled) return;
      setPowerState(status.state);
      setPowerDriver(status.driver);
    }).catch(() => {
      if (cancelled) return;
      setPowerState("unknown");
      setPowerDriver(null);
    }).finally(() => {
      if (!cancelled) setPowerLoading(false);
    });

    return () => { cancelled = true; };
  }, [uuid, hasBmc]);

  const fetchPowerState = () => {
    setPowerLoading(true);
    getDevicePower(uuid).then((status) => {
      setPowerState(status.state);
      setPowerDriver(status.driver);
    }).catch(() => {
      setPowerState("unknown");
      setPowerDriver(null);
    }).finally(() => {
      setPowerLoading(false);
    });
  };

  const executePowerAction = async (action: PowerAction) => {
    setPowerActionInFlight(true);
    onError("");
    try {
      await setDevicePower(uuid, action);
      // Re-fetch state after action; state may take a moment to change but a single fetch is fine
      fetchPowerState();
    } catch (err) {
      onError(err instanceof Error ? err.message : "Failed to execute power action");
    } finally {
      setPowerActionInFlight(false);
    }
  };

  const handlePowerOn = () => {
    executePowerAction("on");
  };

  const handlePowerOff = async () => {
    await executePowerAction("off");
  };

  const handlePowerCycle = async () => {
    await executePowerAction("cycle");
  };

  const requestDestructiveAction = (action: "off" | "cycle") => {
    setPendingAction(action);
    setConfirmDialogOpen(true);
  };

  const handleConfirmDestructive = async () => {
    if (!pendingAction) return;
    if (pendingAction === "off") {
      await handlePowerOff();
    } else {
      await handlePowerCycle();
    }
  };

  if (!hasBmc) return null;

  const buttonsDisabled = powerActionInFlight || powerLoading;

  return (
    <>
      {/* Power State row — rendered inline inside the caller's info grid */}
      <span className="text-muted-foreground">Power State:</span>
      <div className="flex items-center gap-2">
        <PowerStateBadge state={powerState} loading={powerLoading} />
        {!powerLoading && powerDriver && (
          <span className="text-xs text-text-muted">{powerDriver}</span>
        )}
      </div>

      {/* Power Controls section — spans both grid columns when rendered inside the info grid */}
      <div className="col-span-full sm:col-span-2 mt-3 pt-3 border-t border-border-muted">
        <div className="text-xs text-text-secondary uppercase tracking-wide mb-2">Power Controls</div>
        <div className="flex items-center gap-2 flex-wrap">
          <Button
            size="sm"
            variant="secondary"
            onClick={handlePowerOn}
            disabled={buttonsDisabled}
            aria-label="Power on device"
          >
            <Zap className="size-3.5" />
            Power On
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => requestDestructiveAction("off")}
            disabled={buttonsDisabled}
            aria-label="Power off device"
          >
            <Power className="size-3.5" />
            Power Off
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => requestDestructiveAction("cycle")}
            disabled={buttonsDisabled}
            aria-label="Power cycle device"
          >
            <RotateCcw className="size-3.5" />
            Power Cycle
          </Button>
        </div>
      </div>

      {/* Power action confirmation dialog (off / cycle are destructive) */}
      {pendingAction && (
        <PowerConfirmDialog
          open={confirmDialogOpen}
          onOpenChange={(open) => {
            setConfirmDialogOpen(open);
            if (!open) setPendingAction(null);
          }}
          action={pendingAction}
          onConfirm={handleConfirmDestructive}
        />
      )}
    </>
  );
}
