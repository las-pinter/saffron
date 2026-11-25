import { createBrowserRouter, type RouteObject } from 'react-router'
import { Layout } from './Layout'
import { Placeholder } from './Placeholder'

export const routes: RouteObject[] = [
  {
    element: <Layout />,
    children: [
      { index: true, element: <Placeholder /> },
      { path: 'holdings', element: <Placeholder /> },
      { path: 'discover', element: <Placeholder /> },
      { path: 'ai', element: <Placeholder /> },
      { path: '*', element: <Placeholder /> },
    ],
  },
]

export const router = createBrowserRouter(routes)
