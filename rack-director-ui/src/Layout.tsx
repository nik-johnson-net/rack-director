import { Outlet } from "react-router";
import { AppSidebar } from "@/components/app-sidebar";

export default function Layout() {
  return (
    <div className="flex h-screen bg-bg-base">
      <AppSidebar />
      <main className="flex-1 overflow-y-auto p-6 md:p-8">
        <Outlet />
      </main>
    </div>
  );
}
