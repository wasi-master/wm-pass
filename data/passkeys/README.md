# Browsers trusted to speak for a web origin

`privileged.json` is Google's published list of browsers, fetched from `https://www.gstatic.com/gpm-passkeys-privileged-apps/apps.json`. It names each browser's package and signing certificate. WM Pass uses it for two things: a passkey request may state a website's origin only if it comes from one of these, and autofill treats a page's domain as verified only when a browser on this list reports it.
