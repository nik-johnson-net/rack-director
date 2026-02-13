import { Home, Server, Settings, HardDrive, Users, ClipboardList, GitBranch, Network, Layers } from "lucide-react"

import {
  Sidebar,
  SidebarContent,
  SidebarGroup,
  SidebarGroupContent,
  SidebarGroupLabel,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
} from "@/components/ui/sidebar"
import { Link, useMatch } from "react-router";

// Menu items organized by groups
const menuGroups = [
  {
    label: "Infrastructure",
    items: [
      {
        title: "Home",
        url: "/",
        icon: Home,
      },
      {
        title: "Devices",
        url: "/devices",
        icon: Server,
      },
      {
        title: "Networks",
        url: "/networks",
        icon: Network,
      },
      {
        title: "Operating Systems",
        url: "/operating-systems",
        icon: HardDrive,
      },
    ],
  },
  {
    label: "Configuration",
    items: [
      {
        title: "Roles",
        url: "/roles",
        icon: Users,
      },
      {
        title: "Platforms",
        url: "/platforms",
        icon: Layers,
      },
      {
        title: "Plans",
        url: "/plans",
        icon: ClipboardList,
      },
    ],
  },
  {
    label: "Monitoring",
    items: [
      {
        title: "Transitions",
        url: "/transitions",
        icon: GitBranch,
      },
    ],
  },
  {
    label: "System",
    items: [
      {
        title: "Settings",
        url: "/settings",
        icon: Settings,
      },
    ],
  },
]

export function AppSidebar() {
  return (
    <Sidebar>
      <SidebarContent>
        {menuGroups.map((group) => (
          <SidebarGroup key={group.label}>
            <SidebarGroupLabel>{group.label}</SidebarGroupLabel>
            <SidebarGroupContent>
              <SidebarMenu>
                {group.items.map((item) => (
                  <AppSidebarMenuItem key={item.title} {...item} />
                ))}
              </SidebarMenu>
            </SidebarGroupContent>
          </SidebarGroup>
        ))}
      </SidebarContent>
    </Sidebar>
  )
}

function AppSidebarMenuItem({ url, title, icon: Icon }: { url: string; title: string; icon: React.ComponentType }) {
  let isActive = useMatch(url + "/*");
  return (
    <SidebarMenuItem key={title}>
      <SidebarMenuButton asChild isActive={isActive !== null}>
        <Link to={url}>
          <Icon />
          <span>{title}</span>
        </Link>
      </SidebarMenuButton>
    </SidebarMenuItem>
  )
}
