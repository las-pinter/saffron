/** A stubbed WebAuthn browser, for tests that exercise a passkey ceremony. */

import { vi } from 'vitest'

export function bytes(text: string): ArrayBuffer {
  return new TextEncoder().encode(text).buffer as ArrayBuffer
}

/** A browser credential as WebAuthn hands it back after a sign-in. */
export function fakeAssertion() {
  return {
    id: 'credential-1',
    rawId: bytes('raw-id'),
    type: 'public-key',
    getClientExtensionResults: () => ({}),
    response: {
      clientDataJSON: bytes('client'),
      authenticatorData: bytes('auth'),
      signature: bytes('signature'),
      userHandle: null,
    },
  } as unknown as PublicKeyCredential
}

/** A browser credential as WebAuthn hands it back after a registration. */
export function fakeAttestation() {
  return {
    id: 'credential-2',
    rawId: bytes('raw-id'),
    type: 'public-key',
    getClientExtensionResults: () => ({}),
    response: {
      clientDataJSON: bytes('client'),
      attestationObject: bytes('attestation'),
      getTransports: () => ['internal'],
    },
  } as unknown as PublicKeyCredential
}

interface BrowserMethods {
  get?: (options?: unknown) => Promise<PublicKeyCredential | null>
  create?: (options?: unknown) => Promise<PublicKeyCredential | null>
}

const originalCredentials = Object.getOwnPropertyDescriptor(navigator, 'credentials')

/** Makes `window.PublicKeyCredential` and `navigator.credentials` exist for one test. */
export function stubBrowser(methods: BrowserMethods) {
  Object.defineProperty(navigator, 'credentials', { configurable: true, value: methods })
  vi.stubGlobal('PublicKeyCredential', function PublicKeyCredential() {})
}

/** Puts the real navigator back; call this in `afterEach`. */
export function restoreBrowser() {
  vi.unstubAllGlobals()
  if (originalCredentials) {
    Object.defineProperty(navigator, 'credentials', originalCredentials)
  } else {
    delete (navigator as unknown as { credentials?: unknown }).credentials
  }
}
