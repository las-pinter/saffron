/** Passkey ceremonies: the browser's WebAuthn API, in JSON as the server sends it. */

export interface CeremonyOptions {
  [key: string]: unknown
}

/**
 * The server's ceremony options are the browser API's response object, wrapped in a
 * `publicKey` field (`RequestChallengeResponse` / `CreationChallengeResponse` in
 * webauthn-rs).
 */
export interface CeremonyResponse {
  publicKey: CeremonyOptions
}

interface CredentialDescriptorJson {
  id: string
  type?: string
  transports?: string[]
}

function toBytes(value: string): Uint8Array {
  const base64 = value.replaceAll('-', '+').replaceAll('_', '/')
  const padded = base64.padEnd(Math.ceil(base64.length / 4) * 4, '=')
  const binary = atob(padded)
  return Uint8Array.from(binary, (character) => character.charCodeAt(0))
}

function toBuffer(value: string): ArrayBuffer {
  // `toBytes` builds the array with `Uint8Array.from`, so its buffer is never shared.
  return toBytes(value).buffer as ArrayBuffer
}

function encode(value: ArrayBuffer | Uint8Array | null | undefined): string {
  if (!value) return ''
  const bytes = value instanceof Uint8Array ? value : new Uint8Array(value)
  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary).replaceAll('+', '-').replaceAll('/', '_').replaceAll('=', '')
}

function descriptors(value: unknown): PublicKeyCredentialDescriptor[] {
  if (!Array.isArray(value)) return []
  return (value as CredentialDescriptorJson[]).map((descriptor) => ({
    id: toBuffer(descriptor.id),
    type: 'public-key',
    ...(descriptor.transports
      ? { transports: descriptor.transports as AuthenticatorTransport[] }
      : {}),
  }))
}

/** The server's request options as the browser API takes them. */
export function requestOptions(options: CeremonyOptions): PublicKeyCredentialRequestOptions {
  return {
    challenge: toBuffer(options.challenge as string),
    ...(options.timeout === undefined ? {} : { timeout: options.timeout as number }),
    ...(options.rpId === undefined ? {} : { rpId: options.rpId as string }),
    ...(options.userVerification === undefined
      ? {}
      : { userVerification: options.userVerification as UserVerificationRequirement }),
    ...(options.allowCredentials === undefined
      ? {}
      : { allowCredentials: descriptors(options.allowCredentials) }),
    ...(options.extensions === undefined
      ? {}
      : { extensions: options.extensions as AuthenticationExtensionsClientInputs }),
  }
}

/** The server's creation options as the browser API takes them. */
export function creationOptions(options: CeremonyOptions): PublicKeyCredentialCreationOptions {
  const user = options.user as { id: string; name: string; displayName: string }
  const selection = (options.authenticatorSelection ?? {}) as AuthenticatorSelectionCriteria
  return {
    challenge: toBuffer(options.challenge as string),
    rp: options.rp as PublicKeyCredentialRpEntity,
    user: {
      id: toBuffer(user.id),
      name: user.name,
      displayName: user.displayName,
    },
    pubKeyCredParams: options.pubKeyCredParams as PublicKeyCredentialParameters[],
    ...(options.timeout === undefined ? {} : { timeout: options.timeout as number }),
    ...(options.excludeCredentials === undefined
      ? {}
      : { excludeCredentials: descriptors(options.excludeCredentials) }),
    ...(options.attestation === undefined
      ? {}
      : { attestation: options.attestation as AttestationConveyancePreference }),
    ...(options.authenticatorSelection === undefined ? {} : { authenticatorSelection: selection }),
    ...(options.extensions === undefined
      ? {}
      : { extensions: options.extensions as AuthenticationExtensionsClientInputs }),
  }
}

function common(credential: PublicKeyCredential) {
  return {
    id: credential.id,
    rawId: encode(credential.rawId),
    type: credential.type,
    clientExtensionResults: credential.getClientExtensionResults(),
  }
}

/** The assertion the server's sign-in finish call reads. */
export function assertionJson(credential: PublicKeyCredential) {
  const response = credential.response as AuthenticatorAssertionResponse
  return {
    ...common(credential),
    response: {
      clientDataJSON: encode(response.clientDataJSON),
      authenticatorData: encode(response.authenticatorData),
      signature: encode(response.signature),
      userHandle: response.userHandle ? encode(response.userHandle) : null,
    },
  }
}

/** The attestation the server's registration finish call reads. */
export function attestationJson(credential: PublicKeyCredential) {
  const response = credential.response as AuthenticatorAttestationResponse
  return {
    ...common(credential),
    response: {
      clientDataJSON: encode(response.clientDataJSON),
      attestationObject: encode(response.attestationObject),
      transports: response.getTransports?.() ?? [],
    },
  }
}
