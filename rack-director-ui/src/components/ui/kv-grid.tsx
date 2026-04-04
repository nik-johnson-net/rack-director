import { cn } from "@/lib/utils";
import type { ReactNode } from "react";

interface KVItem {
  key: string;
  value: ReactNode;
}

interface KVGridProps {
  items?: KVItem[];
  children?: ReactNode;
  className?: string;
}

export function KVGrid({ items, children, className }: KVGridProps) {
  return (
    <div
      className={cn(
        "grid gap-y-1 gap-x-4",
        className
      )}
      style={{ gridTemplateColumns: "140px 1fr" }}
    >
      {items
        ? items.map((item, i) => (
            <KVRow key={i} label={item.key} value={item.value} />
          ))
        : children}
    </div>
  );
}

interface KVRowProps {
  label: string;
  value: ReactNode;
}

export function KVRow({ label, value }: KVRowProps) {
  return (
    <>
      <dt className="text-xs text-text-secondary uppercase tracking-wide self-start pt-px">
        {label}
      </dt>
      <dd className="text-sm text-text-primary">{value}</dd>
    </>
  );
}
