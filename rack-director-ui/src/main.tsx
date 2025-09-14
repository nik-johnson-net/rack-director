import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'

import { createBrowserRouter } from "react-router";
import { RouterProvider } from "react-router/dom";
import Layout from './Layout.tsx';
import Index from './pages/Index.tsx';
import Devices from './pages/Devices.tsx';
import Plans from './pages/Plans.tsx';
import Transitions from './pages/Transitions.tsx';
import Settings from './pages/Settings.tsx';
import { getDevicesIndex } from './lib/client.ts';
import Loading from './pages/Loading.tsx';

const router = createBrowserRouter([
  {
    Component: Layout,
    children: [
      { index: true, Component: Index, HydrateFallback: Loading },
      { path: "/devices", loader: getDevicesIndex, Component: Devices },
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
