import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { RouterProvider } from 'react-router'
import '@fontsource-variable/inter'
import '@fontsource-variable/inter-tight'
import { AuthProvider } from '@/auth/AuthProvider'
import { i18n } from '@/i18n'
import { router } from '@/app/router'
import './index.css'

const root = document.getElementById('root')
if (!root) throw new Error('index.html has no #root element')

createRoot(root).render(
  <StrictMode>
    <I18nextProvider i18n={i18n}>
      <AuthProvider>
        <RouterProvider router={router} />
      </AuthProvider>
    </I18nextProvider>
  </StrictMode>,
)
