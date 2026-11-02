// Runs in the page's own world so sites reach WM Pass through the ordinary passkey calls. It holds
// nothing: every decision and all signing happen on the other side of the content script. A page
// can already ask for a passkey, so this gives it no new way to reach anything.

export {};

(() => {
  const credentials = navigator.credentials;
  if (!credentials?.get) return;
  const originalGet = credentials.get.bind(credentials);
  const originalCreate = credentials.create.bind(credentials);

  const toB64Url = (buffer: ArrayBuffer | ArrayBufferView) => {
    const bytes = buffer instanceof ArrayBuffer ? new Uint8Array(buffer) : new Uint8Array(buffer.buffer, buffer.byteOffset, buffer.byteLength);
    let text = '';
    for (const b of bytes) text += String.fromCharCode(b);
    return btoa(text).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  };
  const fromB64Url = (text: string) => {
    const raw = atob(text.replace(/-/g, '+').replace(/_/g, '/') + '='.repeat((4 - (text.length % 4)) % 4));
    const out = new Uint8Array(raw.length);
    for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
    return out.buffer;
  };

  const ask = (op: string, options: unknown) =>
    new Promise<any>((resolve) => {
      const id = crypto.randomUUID();
      const listen = (event: MessageEvent) => {
        if (event.source !== window || event.data?.wmpassReply !== id) return;
        window.removeEventListener('message', listen);
        resolve(event.data.answer);
      };
      window.addEventListener('message', listen);
      window.postMessage({ wmpassAsk: id, op, options }, location.origin);
    });

  const makeCredential = (idText: string, response: object) => {
    const rawId = fromB64Url(idText);
    const credential: any = {
      id: idText,
      rawId,
      type: 'public-key',
      authenticatorAttachment: 'platform',
      response,
      getClientExtensionResults: () => ({}),
      toJSON: () => ({ id: idText, rawId: idText, type: 'public-key', authenticatorAttachment: 'platform', clientExtensionResults: {} }),
    };
    if (typeof PublicKeyCredential !== 'undefined') Object.setPrototypeOf(credential, PublicKeyCredential.prototype);
    return credential;
  };

  const cancelled = () => new DOMException('The request was declined.', 'NotAllowedError');

  credentials.get = async function (options?: CredentialRequestOptions) {
    const key = options?.publicKey;
    // A conditional request runs on page load, and nothing here opens by itself.
    if (!key || options?.mediation === 'conditional' || !navigator.userActivation?.isActive) return originalGet(options);
    const answer = await ask('get', {
      challenge: toB64Url(key.challenge as ArrayBuffer),
      rpId: key.rpId,
      allowCredentials: (key.allowCredentials ?? []).map((c) => ({ id: toB64Url(c.id as ArrayBuffer) })),
    });
    if (!answer || answer.fallback) return originalGet(options);
    if (answer.cancelled) throw cancelled();
    return makeCredential(answer.credential_id, {
      clientDataJSON: fromB64Url(answer.clientData),
      authenticatorData: fromB64Url(answer.authenticator_data),
      signature: fromB64Url(answer.signature),
      userHandle: answer.user_handle ? fromB64Url(answer.user_handle) : null,
    });
  };

  credentials.create = async function (options?: CredentialCreationOptions) {
    const key = options?.publicKey;
    if (!key || !navigator.userActivation?.isActive) return originalCreate(options);
    const answer = await ask('create', {
      challenge: toB64Url(key.challenge as ArrayBuffer),
      rp: key.rp,
      user: { id: toB64Url(key.user.id as ArrayBuffer), name: key.user.name, displayName: key.user.displayName },
      pubKeyCredParams: key.pubKeyCredParams,
    });
    if (!answer || answer.fallback) return originalCreate(options);
    if (answer.cancelled) throw cancelled();
    const attestation = fromB64Url(answer.attestation_object);
    const authData = fromB64Url(answer.authenticator_data);
    return makeCredential(answer.credential_id, {
      clientDataJSON: fromB64Url(answer.clientData),
      attestationObject: attestation,
      getAuthenticatorData: () => authData,
      getPublicKey: () => fromB64Url(answer.public_key_der),
      getPublicKeyAlgorithm: () => -7,
      getTransports: () => ['internal', 'hybrid'],
    });
  };
})();
