import { NavLink } from "react-router";
import { LayoutDashboard, Server, Shield, Cpu, HardDrive, Network, Settings } from "lucide-react";

interface NavItem {
  title: string;
  href: string;
  icon: React.ComponentType<{ className?: string }>;
}

const mainNavItems: NavItem[] = [
  { title: "Dashboard",  href: "/",                  icon: LayoutDashboard },
  { title: "Devices",    href: "/devices",            icon: Server          },
  { title: "Roles",      href: "/roles",              icon: Shield          },
  { title: "Platforms",  href: "/platforms",          icon: Cpu             },
  { title: "OS Images",  href: "/operating-systems",  icon: HardDrive       },
  { title: "Networks",   href: "/networks",           icon: Network         },
];

const bottomNavItems: NavItem[] = [
  { title: "Settings", href: "/settings", icon: Settings },
];

function SidebarNavItem({ item }: { item: NavItem }) {
  const Icon = item.icon;
  return (
    <NavLink
      to={item.href}
      end={item.href === "/"}
      className={({ isActive }) =>
        [
          "flex items-center gap-2 py-2 px-5 text-sm transition-colors cursor-pointer",
          "border-l-[3px]",
          isActive
            ? "text-text-primary bg-bg-raised border-accent"
            : "text-text-secondary bg-transparent border-transparent hover:text-text-primary hover:bg-bg-raised",
        ].join(" ")
      }
    >
      <Icon className="w-4 h-4 shrink-0" />
      <span>{item.title}</span>
    </NavLink>
  );
}

export function AppSidebar() {
  return (
    <aside className="flex flex-col w-[200px] shrink-0 h-screen bg-bg-surface border-r border-border">
      {/* Logo */}
      <div className="px-5 pt-5 pb-4">
        <div className="text-xl font-bold text-accent leading-none">RACK</div>
        <div
          className="text-xs text-text-secondary uppercase mt-1"
          style={{ letterSpacing: "3px" }}
        >
          director
        </div>
      </div>

      {/* Separator */}
      <div className="mx-5 border-t border-border" />

      {/* Main nav */}
      <nav className="flex-1 overflow-y-auto py-3">
        {mainNavItems.map((item) => (
          <SidebarNavItem key={item.href} item={item} />
        ))}
      </nav>

      {/* Bottom separator + settings */}
      <div className="mx-5 border-t border-border" />
      <nav className="py-3">
        {bottomNavItems.map((item) => (
          <SidebarNavItem key={item.href} item={item} />
        ))}
      </nav>
    </aside>
  );
}
