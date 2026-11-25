import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { I18nextProvider } from 'react-i18next'
import { createMemoryRouter, RouterProvider } from 'react-router'
import { i18n } from '@/i18n'
import { routes } from './router'

function renderAt(path: string) {
  const router = createMemoryRouter(routes, { initialEntries: [path] })
  render(
    <I18nextProvider i18n={i18n}>
      <RouterProvider router={router} />
    </I18nextProvider>,
  )
  return router
}

describe('Layout', () => {
  it('marks the overview link as current on the root path', () => {
    renderAt('/')

    expect(screen.getByRole('link', { name: 'Overview' })).toHaveAttribute('aria-current', 'page')
  })

  it('moves the current marker when a navigation link is clicked', async () => {
    const router = renderAt('/')

    await userEvent.click(screen.getByRole('link', { name: 'Holdings' }))

    expect(router.state.location.pathname).toBe('/holdings')
    expect(screen.getByRole('link', { name: 'Holdings' })).toHaveAttribute('aria-current', 'page')
  })

  it('renders translated text instead of raw keys', () => {
    renderAt('/')

    expect(screen.getByRole('heading', { name: 'Saffron is being built' })).toBeInTheDocument()
  })
})
