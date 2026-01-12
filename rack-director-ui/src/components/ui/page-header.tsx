import type { ReactNode } from "react";
import { Breadcrumbs } from "./breadcrumbs";
import { cn } from "@/lib/utils";

interface BreadcrumbItem {
  label: string;
  href?: string;
}

interface PageHeaderProps {
  breadcrumbs?: BreadcrumbItem[];
  title: string;
  description?: string;
  status?: ReactNode;
  actions?: ReactNode;
  className?: string;
}

export function PageHeader({
  breadcrumbs,
  title,
  description,
  status,
  actions,
  className,
}: PageHeaderProps) {
  return (
    <div className={cn("space-y-4", className)}>
      {breadcrumbs && breadcrumbs.length > 0 && <Breadcrumbs items={breadcrumbs} />}

      <div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
        <div className="flex-1 space-y-1">
          <div className="flex items-center gap-3">
            <h1 className="text-3xl font-bold tracking-tight">{title}</h1>
            {status && <div className="flex-shrink-0">{status}</div>}
          </div>
          {description && <p className="text-muted-foreground">{description}</p>}
        </div>

        {actions && <div className="flex items-start gap-2 flex-shrink-0">{actions}</div>}
      </div>
    </div>
  );
}
