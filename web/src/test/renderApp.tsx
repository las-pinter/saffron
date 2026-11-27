import { render } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { AuthProvider } from '@/auth/AuthProvider'
import { routes } from '@/app/router'
import { i18n } from '@/i18n'

/** Renders the whole application at `path`, with its real routes and auth state. */
export function renderApp(path = '/') {
  const router = createMemoryRouter(routes, { initialEntries: [path] })
  render(
    <I18nextProvider i18n={i18n}>
      <AuthProvider>
        <RouterProvider router={router} />
      </AuthProvider>
    </I18nextProvider>,
  )
  return router
}
