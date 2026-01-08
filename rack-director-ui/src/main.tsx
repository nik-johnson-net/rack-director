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
import { getAllDevices, getOperatingSystems, getOperatingSystem, getRoles, getRole } from './lib/client.ts';
import Loading from './pages/Loading.tsx';

const router = createBrowserRouter([
  {
    Component: Layout,
    children: [
      { index: true, Component: Index, HydrateFallback: Loading },
      { path: "/devices", loader: getAllDevices, Component: Devices, HydrateFallback: Loading },
      { path: "/devices/:uuid", Component: DeviceDetail },
      { path: "/operating-systems", loader: getOperatingSystems, Component: OperatingSystems, HydrateFallback: Loading },
      { path: "/operating-systems/new", Component: OperatingSystemNew },
      { path: "/operating-systems/:id", loader: ({ params }) => getOperatingSystem(parseInt(params.id!)), Component: OperatingSystemEdit, HydrateFallback: Loading },
      { path: "/roles", loader: getRoles, Component: Roles, HydrateFallback: Loading },
      { path: "/roles/new", Component: RoleNew },
      { path: "/roles/:id", loader: ({ params }) => getRole(parseInt(params.id!)), Component: RoleEdit, HydrateFallback: Loading },
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
