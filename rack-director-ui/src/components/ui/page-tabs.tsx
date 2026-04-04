import { Link } from "react-router";
import { cn } from "@/lib/utils";

interface TabItem {
  label: string;
  value: string;
  href: string;
}

interface PageTabsProps {
  tabs: TabItem[];
  activeTab: string;
  className?: string;
}

export function PageTabs({ tabs, activeTab, className }: PageTabsProps) {
  return (
    <div
      className={cn(
        "flex border-b border-border mb-6",
        className
      )}
    >
      {tabs.map((tab) => {
        const isActive = tab.value === activeTab;
        return (
          <Link
            key={tab.value}
            to={tab.href}
            className={cn(
              "px-4 py-2 text-sm transition-colors border-b-2 -mb-px",
              isActive
                ? "text-text-primary border-accent"
                : "text-text-secondary border-transparent hover:text-text-primary"
            )}
          >
            {tab.label}
          </Link>
        );
      })}
    </div>
  );
}
