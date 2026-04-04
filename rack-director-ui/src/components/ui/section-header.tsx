import { Link } from "react-router";
import { cn } from "@/lib/utils";

interface SectionHeaderProps {
  title: string;
  linkText?: string;
  linkHref?: string;
  className?: string;
}

export function SectionHeader({ title, linkText, linkHref, className }: SectionHeaderProps) {
  return (
    <div className={cn("flex items-center justify-between mb-3", className)}>
      <h2 className="text-lg font-semibold text-text-primary">{title}</h2>
      {linkText && linkHref && (
        <Link
          to={linkHref}
          className="text-xs text-accent hover:text-accent-hover transition-colors"
        >
          {linkText}
        </Link>
      )}
    </div>
  );
}
