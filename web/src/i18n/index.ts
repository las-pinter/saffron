import { createInstance } from 'i18next'
import { initReactI18next } from 'react-i18next'
import en from './en.json'

// English is the only language in v1; every UI string still goes through a key so
// further languages need only a new resource file.
export const resources = { en: { translation: en } } as const

export const i18n = createInstance()

void i18n.use(initReactI18next).init({
  resources,
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
})
