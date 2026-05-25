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

interface CancelTransitionDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => Promise<void>;
}

export function CancelTransitionDialog({
  open,
  onOpenChange,
  onConfirm,
}: CancelTransitionDialogProps) {
  const [isCancelling, setIsCancelling] = useState(false);

  const handleConfirm = async () => {
    setIsCancelling(true);
    try {
      await onConfirm();
      onOpenChange(false);
    } finally {
      setIsCancelling(false);
    }
  };

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Cancel in-progress transition?</AlertDialogTitle>
          <AlertDialogDescription>
            This will stop the current lifecycle transition immediately.
            <span className="block mt-2 font-semibold text-status-broken">
              Warning: Cancelling a transition may leave the device in a broken
              state. The device will require manual repair before it can be used
              again.
            </span>
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isCancelling}>
            Keep Running
          </AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            disabled={isCancelling}
            className="bg-destructive hover:bg-destructive/90 text-destructive-foreground"
          >
            {isCancelling ? "Cancelling..." : "Cancel Transition"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
