// Each app's export steps, read from the website's guide pages bundled with the app, so following
// one does not mean leaving for the browser. Pictures stay on the vendor's site, a click away.

export const GUIDES = 'https://wmpass.pages.dev/guides/import/';

/** The guides, with the mark the vault would give each app. */
export const GUIDE_LIST: { page: string; name: string; brand?: string }[] = [
  { page: '1password', name: '1Password', brand: '1password' },
  { page: 'apple-passwords', name: 'Apple Passwords', brand: 'apple' },
  { page: 'avast-passwords', name: 'Avast Passwords', brand: 'avast' },
  { page: 'avira-password-manager', name: 'Avira Password Manager', brand: 'avira' },
  { page: 'bitwarden', name: 'Bitwarden', brand: 'bitwarden' },
  { page: 'brave', name: 'Brave', brand: 'brave' },
  { page: 'buttercup', name: 'Buttercup' },
  { page: 'dashlane', name: 'Dashlane', brand: 'dashlane' },
  { page: 'enpass', name: 'Enpass', brand: 'enpass' },
  { page: 'firefox', name: 'Firefox', brand: 'firefox' },
  { page: 'chrome', name: 'Google Password Manager', brand: 'googlechrome' },
  { page: 'kaspersky-password-manager', name: 'Kaspersky Password Manager', brand: 'kaspersky' },
  { page: 'keepass', name: 'KeePass and KeePassXC', brand: 'keepassxc' },
  { page: 'keepass2android', name: 'Keepass2Android' },
  { page: 'keepassdx', name: 'KeePassDX' },
  { page: 'keeper', name: 'Keeper', brand: 'keeper' },
  { page: 'lastpass', name: 'LastPass', brand: 'lastpass' },
  { page: 'microsoft-edge', name: 'Microsoft Edge' },
  { page: 'msecure', name: 'mSecure' },
  { page: 'nordpass', name: 'NordPass' },
  { page: 'opera', name: 'Opera', brand: 'opera' },
  { page: 'password-safe', name: 'Password Safe' },
  { page: 'proton-pass', name: 'Proton Pass', brand: 'proton' },
  { page: 'psono', name: 'Psono' },
  { page: 'roboform', name: 'RoboForm' },
  { page: 'safeincloud', name: 'SafeInCloud' },
  { page: 'sticky-password', name: 'Sticky Password' },
  { page: 'zoho-vault', name: 'Zoho Vault', brand: 'zoho' },
  { page: 'google-authenticator', name: 'Google Authenticator', brand: 'googleauthenticator' },
  { page: '2fas', name: '2FAS', brand: '2fas' },
  { page: 'aegis', name: 'Aegis', brand: 'aegisauthenticator' },
  { page: 'andotp', name: 'andOTP' },
  { page: 'authenticator-pro', name: 'Authenticator Pro' },
  { page: 'bitwarden-authenticator', name: 'Bitwarden Authenticator', brand: 'bitwarden' },
  { page: 'ente-auth', name: 'Ente Auth', brand: 'ente' },
  { page: 'freeotp-plus', name: 'FreeOTP+' },
  { page: 'lastpass-authenticator', name: 'LastPass Authenticator', brand: 'lastpass' },
  { page: 'proton-authenticator', name: 'Proton Authenticator', brand: 'proton' },
  { page: 'raivo', name: 'Raivo' },
];

/** Bold, code and links: all the guides use. A link to the site's own pages stays plain text. */
export type Run = { t: 'text' | 'bold' | 'code'; s: string } | { t: 'link'; s: string; url: string };

export type Block =
  | { t: 'heading'; text: string; tab: string | null }
  | { t: 'paragraph'; runs: Run[]; tab: string | null }
  | { t: 'bullet'; runs: Run[]; tab: string | null }
  | { t: 'step'; number: string; runs: Run[]; tab: string | null }
  | { t: 'note'; title: string; runs: Run[]; tab: string | null }
  | { t: 'picture'; caption: string; publisher: string; src: string; tab: string | null };

export interface GuidePage {
  title: string;
  blocks: Block[];
  /** The device tabs, in the page's order. */
  tabs: string[];
}

// The pages themselves, loaded only when one is opened.
const SOURCES = import.meta.glob('../../../docs/src/content/docs/guides/import/*.mdx', { query: '?raw', import: 'default' }) as Record<
  string,
  () => Promise<string>
>;

export async function loadGuide(page: string): Promise<GuidePage | null> {
  const load = Object.entries(SOURCES).find(([path]) => path.endsWith(`/${page}.mdx`))?.[1];
  if (!load) return null;
  try {
    return readGuide(await load());
  } catch {
    return null;
  }
}

function attribute(tag: string, name: string): string {
  return new RegExp(`\\b${name}="([^"]*)"`).exec(tag)?.[1] ?? '';
}

/** The website's MDX, as far as its import guides use it: tabs, steps, asides and pictures. */
export function readGuide(mdx: string): GuidePage {
  let lines = mdx.split('\n');
  let title = 'Export guide';
  if (lines[0]?.trim() === '---') {
    const end = lines.slice(1).findIndex((l) => l.trim() === '---') + 1;
    const line = lines.slice(1, end).find((l) => l.startsWith('title:'));
    if (line) title = line.slice('title:'.length).trim().replace(/^"|"$/g, '');
    lines = lines.slice(end + 1);
  }
  const blocks: Block[] = [];
  let paragraph = '';
  let tab: string | null = null;
  let aside: { title: string; text: string } | null = null;
  const flush = () => {
    if (paragraph.trim()) blocks.push({ t: 'paragraph', runs: inline(paragraph.trim()), tab });
    paragraph = '';
  };
  for (const raw of lines) {
    const line = raw.trim();
    if (line.startsWith('import ')) continue;
    if (line.startsWith('<Aside')) {
      flush();
      aside = { title: attribute(line, 'title'), text: '' };
    } else if (line.startsWith('</Aside>') && aside) {
      blocks.push({ t: 'note', title: aside.title, runs: inline(aside.text.trim()), tab });
      aside = null;
    } else if (aside) {
      aside.text += `${line} `;
    } else if (line.startsWith('<TabItem')) {
      flush();
      tab = attribute(line, 'label');
    } else if (line.startsWith('</TabItem>')) {
      flush();
      tab = null;
    } else if (line.startsWith('<Shot')) {
      flush();
      blocks.push({ t: 'picture', caption: attribute(line, 'alt'), publisher: attribute(line, 'publisher'), src: attribute(line, 'src'), tab });
    } else if (line.startsWith('<')) {
      // The wrappers themselves carry nothing to show.
      flush();
    } else if (line.startsWith('## ') || line.startsWith('### ')) {
      flush();
      blocks.push({ t: 'heading', text: line.replace(/^#+/, '').trim(), tab });
    } else if (/^\d+\.\s/.test(line)) {
      flush();
      const dot = line.indexOf('.');
      blocks.push({ t: 'step', number: line.slice(0, dot), runs: inline(line.slice(dot + 1).trim()), tab });
    } else if (line.startsWith('- ')) {
      flush();
      blocks.push({ t: 'bullet', runs: inline(line.slice(2)), tab });
    } else if (line === '') {
      flush();
    } else {
      paragraph += `${line} `;
    }
  }
  flush();
  const tabs = [...new Set(blocks.map((b) => b.tab).filter((t): t is string => t !== null))];
  return { title, blocks, tabs };
}

export function inline(text: string): Run[] {
  const runs: Run[] = [];
  let plain = '';
  const push = (run: Run) => {
    if (plain) runs.push({ t: 'text', s: plain });
    plain = '';
    runs.push(run);
  };
  let i = 0;
  while (i < text.length) {
    if (text.startsWith('**', i) && text.indexOf('**', i + 2) > 0) {
      const end = text.indexOf('**', i + 2);
      push({ t: 'bold', s: text.slice(i + 2, end) });
      i = end + 2;
    } else if (text[i] === '`' && text.indexOf('`', i + 1) > 0) {
      const end = text.indexOf('`', i + 1);
      push({ t: 'code', s: text.slice(i + 1, end) });
      i = end + 1;
    } else if (text[i] === '[' && text.indexOf('](', i) > 0 && text.indexOf(')', text.indexOf('](', i)) > 0) {
      const middle = text.indexOf('](', i);
      const end = text.indexOf(')', middle);
      const url = text.slice(middle + 2, end);
      const label = text.slice(i + 1, middle);
      if (url.startsWith('https://')) push({ t: 'link', s: label, url });
      else plain += label;
      i = end + 1;
    } else {
      plain += text[i];
      i++;
    }
  }
  if (plain) runs.push({ t: 'text', s: plain });
  return runs;
}

export function publisherSite(publisher: string): string {
  return publisher.endsWith('s') ? `${publisher}' site` : `${publisher}'s site`;
}
