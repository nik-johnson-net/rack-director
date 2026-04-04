interface AlertBannerProps {
  variant: "success" | "error" | "warning";
  message: string | null | undefined;
}

export function AlertBanner({ variant, message }: AlertBannerProps) {
  if (!message) return null;

  const styles =
    variant === "success"
      ? "bg-status-provisioned-bg border-l-2 border-status-provisioned text-status-provisioned"
      : variant === "warning"
      ? "bg-warn-bg border-l-2 border-warn-border text-status-unprovisioned"
      : "bg-error-bg border-l-2 border-error-border text-status-broken";

  return (
    <div className={`${styles} px-3 py-2 text-xs`}>{message}</div>
  );
}
