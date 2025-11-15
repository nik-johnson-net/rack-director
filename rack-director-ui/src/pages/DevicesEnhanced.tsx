import { useState, useEffect } from "react";
import DevicesTableEnhanced from "@/components/devices/devices-table-enhanced";
import type { Device, DhcpLease, RoleWithOs } from "@/lib/client";
import { useLoaderData } from "react-router";
import { getDhcpLeases, getRoles } from "@/lib/client";

function DevicesEnhanced() {
  const initialDevices = useLoaderData<Device[]>();
  const [dhcpLeases, setDhcpLeases] = useState<DhcpLease[]>([]);
  const [roles, setRoles] = useState<RoleWithOs[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    const fetchData = async () => {
      try {
        const [leasesData, rolesData] = await Promise.all([
          getDhcpLeases(),
          getRoles()
        ]);
        setDhcpLeases(leasesData);
        setRoles(rolesData);
      } catch (error) {
        console.error("Failed to load additional data:", error);
      } finally {
        setLoading(false);
      }
    };
    fetchData();
  }, []);

  // Create roles map for quick lookup
  const rolesMap = new Map(
    roles.map(role => [
      role.id!,
      { name: role.name, os_name: role.os_name, os_version: role.os_version }
    ])
  );

  if (loading) {
    return <div className="p-4">Loading device information...</div>;
  }

  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h1 className="text-3xl font-bold">Devices</h1>
        <div className="text-sm text-gray-600">
          {initialDevices.length} device{initialDevices.length !== 1 ? 's' : ''}
        </div>
      </div>
      <DevicesTableEnhanced
        data={initialDevices}
        dhcpLeases={dhcpLeases}
        rolesMap={rolesMap}
      />
    </div>
  );
}

export default DevicesEnhanced;
