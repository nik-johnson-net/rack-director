interface AlertBannerProps {
  variant: "success" | "error";
  message: string | null | undefined;
}

export function AlertBanner({ variant, message }: AlertBannerProps) {
  if (!message) return null;

  const styles =
    variant === "success"
      ? "bg-green-500/10 border border-green-500 text-green-700 dark:text-green-400"
      : "bg-destructive/10 border border-destructive text-destructive";

  return (
    <div className={`${styles} px-4 py-3 rounded-md`}>{message}</div>
  );
}
