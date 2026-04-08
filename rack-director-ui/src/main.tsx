import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'

import { createBrowserRouter } from "react-router";
import { RouterProvider } from "react-router/dom";
import Layout from './Layout.tsx';
import Index from './pages/Index.tsx';
import Devices from './pages/Devices.tsx';
import DeviceDetail from './pages/DeviceDetail.tsx';
import Plans from './pages/Plans.tsx';
import Transitions from './pages/Transitions.tsx';
import Settings from './pages/Settings.tsx';
import OperatingSystems from './pages/OperatingSystems.tsx';
import OperatingSystemNew from './pages/OperatingSystemNew.tsx';
import OperatingSystemEdit from './pages/OperatingSystemEdit.tsx';
import Roles from './pages/Roles.tsx';
import RoleNew from './pages/RoleNew.tsx';
import RoleEdit from './pages/RoleEdit.tsx';
import Networks from './pages/Networks.tsx';
import NetworkNew from './pages/NetworkNew.tsx';
import NetworkDetail from './pages/NetworkDetail.tsx';
import Platforms from './pages/Platforms.tsx';
import PlatformNew from './pages/PlatformNew.tsx';
import PlatformDetail from './pages/PlatformDetail.tsx';
import PlatformEdit from './pages/PlatformEdit.tsx';
import PendingDeviceNew from './pages/PendingDeviceNew.tsx';
import OsmModules from './pages/OsmModules.tsx';
import OsmUpload from './pages/OsmUpload.tsx';
import OsmModuleDetail from './pages/OsmModuleDetail.tsx';
import { getAllDevices, getOperatingSystems, getOperatingSystem, getRoles, getRole, getNetworks, getNetwork, getPoolsForNetwork, getStaticReservations, getLeasesForNetwork, getDhcpLeases, getPendingDevices, getPlatforms, getPlatform, getPlatformDevicesWithDetails, getOsmModules, getOsmUploads, getOsmModule, getOsmModuleOperatingSystems } from './lib/client.ts';
import Loading from './pages/Loading.tsx';

const router = createBrowserRouter([
  {
    Component: Layout,
    children: [
      {
        index: true,
        loader: async () => {
          const devices = await getAllDevices();
          return { devices };
        },
        Component: Index,
        HydrateFallback: Loading
      },
      {
        path: "/devices",
        loader: async () => {
          const [devices, dhcpLeases, pendingDevices, platforms, roles] = await Promise.all([
            getAllDevices(),
            getDhcpLeases(),
            getPendingDevices(),
            getPlatforms(),
            getRoles(),
          ]);
          return { devices, dhcpLeases, pendingDevices, platforms, roles };
        },
        Component: Devices,
        HydrateFallback: Loading
      },
      { path: "/devices/pending/new", Component: PendingDeviceNew },
      { path: "/devices/:uuid", Component: DeviceDetail },
      { path: "/operating-systems", loader: getOperatingSystems, Component: OperatingSystems, HydrateFallback: Loading },
      { path: "/operating-systems/new", Component: OperatingSystemNew },
      { path: "/operating-systems/:id", loader: ({ params }) => getOperatingSystem(parseInt(params.id!)), Component: OperatingSystemEdit, HydrateFallback: Loading },
      { path: "/roles", loader: getRoles, Component: Roles, HydrateFallback: Loading },
      { path: "/roles/new", Component: RoleNew },
      { path: "/roles/:id", loader: ({ params }) => getRole(parseInt(params.id!)), Component: RoleEdit, HydrateFallback: Loading },
      { path: "/platforms", loader: getPlatforms, Component: Platforms, HydrateFallback: Loading },
      { path: "/platforms/new", Component: PlatformNew },
      {
        path: "/platforms/:id",
        loader: async ({ params }) => {
          const id = parseInt(params.id!);
          const [platform, devices] = await Promise.all([
            getPlatform(id),
            getPlatformDevicesWithDetails(id),
          ]);
          return { platform, devices };
        },
        Component: PlatformDetail,
        HydrateFallback: Loading
      },
      {
        path: "/platforms/:id/edit",
        loader: ({ params }) => getPlatform(parseInt(params.id!)),
        Component: PlatformEdit,
        HydrateFallback: Loading
      },
      { path: "/osm/upload", Component: OsmUpload },
      {
        path: "/osm/:id",
        loader: async ({ params }) => {
          const id = parseInt(params.id!);
          const [module, operatingSystems] = await Promise.all([
            getOsmModule(id),
            getOsmModuleOperatingSystems(id),
          ]);
          return { module, operatingSystems };
        },
        Component: OsmModuleDetail,
        HydrateFallback: Loading
      },
      {
        path: "/osm",
        loader: async () => {
          const [modules, uploads] = await Promise.all([
            getOsmModules(),
            getOsmUploads(),
          ]);
          return { modules, uploads };
        },
        Component: OsmModules,
        HydrateFallback: Loading
      },
      { path: "/networks", loader: getNetworks, Component: Networks, HydrateFallback: Loading },
      { path: "/networks/new", Component: NetworkNew },
      {
        path: "/networks/:id",
        loader: async ({ params }) => {
          const networkId = parseInt(params.id!);
          const [network, pools, reservations, leases, pendingDevices] = await Promise.all([
            getNetwork(networkId),
            getPoolsForNetwork(networkId),
            getStaticReservations(networkId),
            getLeasesForNetwork(networkId),
            getPendingDevices(),
          ]);
          return { network, pools, reservations, leases, pendingDevices };
        },
        Component: NetworkDetail,
        HydrateFallback: Loading
      },
      { path: "/plans", Component: Plans },
      { path: "/transitions", Component: Transitions },
      { path: "/settings", Component: Settings },
    ]
  }
]);

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <RouterProvider router={router} />
  </StrictMode>,
)
