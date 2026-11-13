// Runs in the page's own world so sites reach WM Pass through the ordinary passkey calls. It holds
// nothing: every decision and all signing happen on the other side of the content script. A page
// can already ask for a passkey, so this gives it no new way to reach anything.

export {};

(() => {
  const credentials = navigator.credentials;
  // A sandboxed frame has no origin to post to, so asking would throw where the browser's own
  // answer belongs. Its calls are left as the browser made them.
  if (!credentials?.get || location.origin === 'null') return;
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

  const aborted = (signal?: AbortSignal) => signal?.reason ?? new DOMException('The operation was aborted.', 'AbortError');

  // A site that gives up, as when the person leaves its sign-in for another way, aborts the call:
  // the promise ends at once, and the window asking the person is put away.
  const ask = (op: string, options: unknown, signal?: AbortSignal) =>
    new Promise<any>((resolve, reject) => {
      const id = crypto.randomUUID();
      // The content script says at once that it heard. With none listening, as after the
      // extension updates, the browser's own passkey prompt takes over rather than a hang.
      const unheard = setTimeout(() => finish({ fallback: true }), 1500);
      const stop = () => {
        clearTimeout(unheard);
        window.removeEventListener('message', listen);
        signal?.removeEventListener('abort', abort);
      };
      const finish = (answer: unknown) => {
        stop();
        resolve(answer);
      };
      const abort = () => {
        stop();
        window.postMessage({ wmpassCancel: id }, location.origin);
        reject(aborted(signal));
      };
      const listen = (event: MessageEvent) => {
        if (event.source !== window) return;
        if (event.data?.wmpassAck === id) clearTimeout(unheard);
        else if (event.data?.wmpassReply === id) finish(event.data.answer);
      };
      window.addEventListener('message', listen);
      signal?.addEventListener('abort', abort, { once: true });
      window.postMessage({ wmpassAsk: id, op, options }, location.origin);
    });

  // The JSON form carries the response too, as the browser's own does: sites that send
  // JSON.stringify(credential) to their server read it from there.
  const makeCredential = (idText: string, response: object, responseJson: object, extensions: object = {}) => {
    const rawId = fromB64Url(idText);
    const credential: any = {
      id: idText,
      rawId,
      type: 'public-key',
      authenticatorAttachment: 'platform',
      response,
      getClientExtensionResults: () => ({ ...extensions }),
      toJSON: () => ({ id: idText, rawId: idText, type: 'public-key', authenticatorAttachment: 'platform', response: responseJson, clientExtensionResults: { ...extensions } }),
    };
    if (typeof PublicKeyCredential !== 'undefined') Object.setPrototypeOf(credential, PublicKeyCredential.prototype);
    return credential;
  };

  const cancelled = () => new DOMException('The request was declined.', 'NotAllowedError');
  // As the browser's own says when the authenticator already holds one of the excluded credentials.
  const excluded = () => new DOMException('A passkey for this account is already saved in WM Pass.', 'InvalidStateError');

  credentials.get = async function (options?: CredentialRequestOptions) {
    const key = options?.publicKey;
    // A conditional request runs on page load, and nothing here opens by itself.
    if (!key || options?.mediation === 'conditional' || !navigator.userActivation?.isActive) return originalGet(options);
    if (options?.signal?.aborted) throw aborted(options.signal);
    const answer = await ask('get', {
      challenge: toB64Url(key.challenge as ArrayBuffer),
      rpId: key.rpId,
      allowCredentials: (key.allowCredentials ?? []).map((c) => ({ id: toB64Url(c.id as ArrayBuffer) })),
      userVerification: key.userVerification,
    }, options?.signal);
    if (!answer || answer.fallback) return originalGet(options);
    if (answer.cancelled) throw cancelled();
    const response = {
      clientDataJSON: fromB64Url(answer.clientData),
      authenticatorData: fromB64Url(answer.authenticator_data),
      signature: fromB64Url(answer.signature),
      userHandle: answer.user_handle ? fromB64Url(answer.user_handle) : null,
    };
    return makeCredential(answer.credential_id, response, {
      clientDataJSON: toB64Url(response.clientDataJSON),
      authenticatorData: toB64Url(response.authenticatorData),
      signature: toB64Url(response.signature),
      ...(response.userHandle ? { userHandle: toB64Url(response.userHandle) } : {}),
    });
  };

  credentials.create = async function (options?: CredentialCreationOptions) {
    const key = options?.publicKey;
    // A site that asks for a key you carry, as when adding a security key, means that key, not a
    // passkey kept in the vault.
    if (!key || !navigator.userActivation?.isActive || key.authenticatorSelection?.authenticatorAttachment === 'cross-platform') return originalCreate(options);
    if (options?.signal?.aborted) throw aborted(options.signal);
    const answer = await ask('create', {
      challenge: toB64Url(key.challenge as ArrayBuffer),
      rp: key.rp,
      user: { id: toB64Url(key.user.id as ArrayBuffer), name: key.user.name, displayName: key.user.displayName },
      pubKeyCredParams: key.pubKeyCredParams,
      userVerification: key.authenticatorSelection?.userVerification,
      // The ones the site already has for this account: a passkey here among them is refused.
      excludeCredentials: (key.excludeCredentials ?? []).map((c) => ({ id: toB64Url(c.id as ArrayBuffer) })),
    }, options?.signal);
    if (!answer || answer.fallback) return originalCreate(options);
    if (answer.cancelled) throw cancelled();
    if (answer.excluded) throw excluded();
    const attestation = fromB64Url(answer.attestation_object);
    const authData = fromB64Url(answer.authenticator_data);
    const clientDataJSON = fromB64Url(answer.clientData);
    const publicKey = fromB64Url(answer.public_key_der);
    return makeCredential(
      answer.credential_id,
      {
        clientDataJSON,
        attestationObject: attestation,
        getAuthenticatorData: () => authData,
        getPublicKey: () => publicKey,
        getPublicKeyAlgorithm: () => -7,
        getTransports: () => ['internal', 'hybrid'],
      },
      {
        clientDataJSON: toB64Url(clientDataJSON),
        authenticatorData: toB64Url(authData),
        transports: ['internal', 'hybrid'],
        publicKey: toB64Url(publicKey),
        publicKeyAlgorithm: -7,
        attestationObject: toB64Url(attestation),
      },
      // A passkey kept in the vault is always discoverable, which sites asking credProps read.
      (key.extensions as any)?.credProps ? { credProps: { rk: true } } : {},
    );
  };
})();
