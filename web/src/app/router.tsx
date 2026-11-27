import { createBrowserRouter, type RouteObject } from 'react-router'
import { RequireAuth, RequireGuest, RequireSetup } from '@/auth/RequireAuth'
import { LoginPage } from '@/pages/LoginPage'
import { SettingsPage } from '@/pages/SettingsPage'
import { SetupPage } from '@/pages/SetupPage'
import { Layout } from './Layout'
import { Placeholder } from './Placeholder'

export const routes: RouteObject[] = [
  {
    element: <RequireGuest />,
    children: [{ path: 'login', element: <LoginPage /> }],
  },
  {
    element: <RequireSetup />,
    children: [{ path: 'setup', element: <SetupPage /> }],
  },
  {
    element: <RequireAuth />,
    children: [
      {
        element: <Layout />,
        children: [
          { index: true, element: <Placeholder /> },
          { path: 'holdings', element: <Placeholder /> },
          { path: 'discover', element: <Placeholder /> },
          { path: 'ai', element: <Placeholder /> },
          { path: 'settings', element: <SettingsPage /> },
          { path: '*', element: <Placeholder /> },
        ],
      },
    ],
  },
]

export const router = createBrowserRouter(routes)
