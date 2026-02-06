import { useState } from "react";
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
import type { DeviceLifecycle } from "@/lib/client";

interface TransitionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  currentState: DeviceLifecycle;
  targetState: DeviceLifecycle;
  onConfirm: () => Promise<void>;
}

export function TransitionDialog({
  open,
  onOpenChange,
  currentState,
  targetState,
  onConfirm,
}: TransitionDialogProps) {
  const [isTransitioning, setIsTransitioning] = useState(false);

  const handleConfirm = async () => {
    setIsTransitioning(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } finally {
      setIsTransitioning(false);
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Transition to {targetState}?</AlertDialogTitle>
          <AlertDialogDescription>
            This will transition the device from "{currentState}" to "{targetState}".
            A lifecycle transition plan will be created.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isTransitioning}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={isTransitioning}
          >
            {isTransitioning ? "Transitioning..." : "Transition"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
