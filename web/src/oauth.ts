// Where a service's sign-in page sends its answer for the web vault. The answer goes to the vault's
// own tab over a channel only this site's pages can hear, and the window closes. The code in it is
// useless without the verifier, which never left the vault's tab.

const channel = new BroadcastChannel('wmpass-oauth');
channel.postMessage({ url: location.href });
channel.close();
history.replaceState(null, '', location.pathname);
document.getElementById('note')!.textContent = 'Signed in. You can close this window.';
window.close();
