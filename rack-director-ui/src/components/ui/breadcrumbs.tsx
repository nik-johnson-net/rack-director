import { Link } from "react-router";
import { cn } from "@/lib/utils";

interface BreadcrumbItem {
  label: string;
  href?: string;
}

interface BreadcrumbsProps {
  items: BreadcrumbItem[];
  className?: string;
}

export function Breadcrumbs({ items, className }: BreadcrumbsProps) {
  return (
    <nav aria-label="Breadcrumb" className={cn("flex items-center gap-1 text-xs text-text-muted mb-3", className)}>
      {items.map((item, index) => {
        const isLast = index === items.length - 1;

        return (
          <span key={index} className="flex items-center gap-1">
            {index > 0 && <span className="text-text-muted">/</span>}
            {isLast || !item.href ? (
              <span className="text-text-muted">{item.label}</span>
            ) : (
              <Link
                to={item.href}
                className="text-text-secondary hover:text-accent transition-colors"
              >
                {item.label}
              </Link>
            )}
          </span>
        );
      })}
    </nav>
  );
}
