// Every kind of place WM Pass can keep copies in, the same list as WM Keyboard's backups, and the
// services whose addresses WebDAV and S3 can fill in. Which kinds a shell offers is its own call
// (`host().places.kinds`): a browser cannot open a raw connection, so it has no SFTP or SMB.

export interface PlaceKind {
  id: string;
  name: string;
  blurb: string;
  group: 0 | 1 | 2 | 3;
  /** Signs in on the service's own page, in the browser. */
  browser?: boolean;
  /** Signs in here with a password, maybe a code after it. */
  password?: boolean;
}

export const GROUPS = ['On this device', 'Accounts', 'Servers and storage', 'Somewhere else'];

export const PLACE_KINDS: PlaceKind[] = [
  { id: 'folder', name: 'Folder', blurb: 'A folder on this computer', group: 0 },
  { id: 'drive', name: 'Google Drive', blurb: 'Google Drive', group: 1, browser: true },
  { id: 'dropbox', name: 'Dropbox', blurb: 'Dropbox', group: 1, browser: true },
  { id: 'onedrive', name: 'OneDrive', blurb: 'OneDrive', group: 1, browser: true },
  { id: 'box', name: 'Box', blurb: 'Box', group: 1, browser: true },
  { id: 'pcloud', name: 'pCloud', blurb: 'pCloud', group: 1, password: true },
  { id: 'mega', name: 'MEGA', blurb: 'MEGA', group: 1, password: true },
  { id: 'icloud', name: 'iCloud Drive', blurb: 'iCloud Drive', group: 1, password: true },
  { id: 'proton', name: 'Proton Drive', blurb: 'Proton Drive', group: 1, password: true },
  { id: 'webdav', name: 'WebDAV', blurb: 'WebDAV, such as Nextcloud', group: 2 },
  { id: 's3', name: 'S3', blurb: 'S3, such as Backblaze or MinIO', group: 2 },
  { id: 'sftp', name: 'SFTP', blurb: 'SFTP server', group: 2 },
  { id: 'ftp', name: 'FTP', blurb: 'FTP server', group: 2 },
  { id: 'smb', name: 'SMB', blurb: 'Windows or Samba share', group: 2 },
  { id: 'nfs', name: 'NFS', blurb: 'NFS export', group: 2 },
  { id: 'synology', name: 'Synology', blurb: 'Synology NAS', group: 2, password: true },
  { id: 'qnap', name: 'QNAP', blurb: 'QNAP NAS', group: 2, password: true },
  { id: 'git', name: 'Git', blurb: 'A Git repository', group: 3 },
  { id: 'imap', name: 'Email', blurb: 'A mailbox, over IMAP', group: 3 },
];

export function kindOf(id: string) {
  return PLACE_KINDS.find((k) => k.id === id);
}

export const DEFAULT_FOLDER = 'WM Pass';

export interface WebDavPreset {
  id: string;
  label: string;
  template: string;
  /** What `{server}` is, when the address has one. */
  server?: 'host' | 'account' | 'taildrive';
}

export const WEBDAV_PRESETS: WebDavPreset[] = [
  { id: 'nextcloud', label: 'Nextcloud', template: 'https://{server}/remote.php/webdav/{folder}', server: 'host' },
  { id: 'owncloud', label: 'ownCloud', template: 'https://{server}/remote.php/webdav/{folder}', server: 'host' },
  { id: 'seafile', label: 'Seafile', template: 'https://{server}/seafdav/{folder}', server: 'host' },
  { id: 'koofr', label: 'Koofr', template: 'https://app.koofr.net/dav/Koofr/{folder}' },
  { id: 'pcloud-us', label: 'pCloud (US)', template: 'https://webdav.pcloud.com/{folder}' },
  { id: 'pcloud-eu', label: 'pCloud (EU)', template: 'https://ewebdav.pcloud.com/{folder}' },
  { id: 'yandex', label: 'Yandex Disk', template: 'https://webdav.yandex.com/{folder}' },
  { id: 'kdrive', label: 'kDrive', template: 'https://{server}.connect.kdrive.infomaniak.com/{folder}', server: 'account' },
  { id: 'storagebox', label: 'Hetzner Storage Box', template: 'https://{user}.your-storagebox.de/{folder}' },
  { id: '4shared', label: '4shared', template: 'https://webdav.4shared.com/{folder}' },
  { id: 'taildrive', label: 'Tailscale Taildrive', template: 'http://100.100.100.100:8080/{server}/{folder}', server: 'taildrive' },
];

const segment = (part: string) => encodeURIComponent(part.trim());
const pathOf = (path: string) => path.trim().replace(/^\/+|\/+$/g, '').split('/').filter(Boolean).map(segment).join('/');

/** The address for a preset, each part percent-encoded segment by segment. */
export function webDavUrl(preset: WebDavPreset, server: string, user: string, folder: string) {
  const host = server.trim().replace(/^https?:\/\//, '').replace(/^\/+|\/+$/g, '');
  return (
    preset.template
      .replace('{server}', preset.server === 'taildrive' ? pathOf(host) : host)
      .replace('{user}', segment(user))
      .replace('{folder}', pathOf(folder || DEFAULT_FOLDER))
      .replace(/\/+$/, '') + '/'
  );
}

export interface S3Preset {
  id: string;
  label: string;
  template: string;
  region: string;
  pathStyle?: boolean;
  regions?: string[];
  /** What `{account}` is, when the endpoint has one. */
  account?: 'account_id' | 'endpoint' | 'server';
}

export const S3_PRESETS: S3Preset[] = [
  { id: 'aws', label: 'Amazon S3', template: '', region: 'us-east-1', regions: ['us-east-1', 'us-west-2', 'eu-west-1', 'eu-central-1', 'ap-south-1', 'ap-southeast-1'] },
  { id: 'r2', label: 'Cloudflare R2', template: 'https://{account}.r2.cloudflarestorage.com', region: 'auto', account: 'account_id' },
  { id: 'b2', label: 'Backblaze B2', template: 'https://s3.{region}.backblazeb2.com', region: 'us-west-004', regions: ['us-west-000', 'us-west-001', 'us-west-002', 'us-west-004', 'us-east-005', 'eu-central-003'] },
  { id: 'wasabi', label: 'Wasabi', template: 'https://s3.{region}.wasabisys.com', region: 'us-east-1', regions: ['us-east-1', 'us-east-2', 'us-west-1', 'eu-central-1', 'eu-west-1', 'ap-northeast-1'] },
  { id: 'gcs', label: 'Google Cloud Storage', template: 'https://storage.googleapis.com', region: 'auto' },
  { id: 'spaces', label: 'DigitalOcean Spaces', template: 'https://{region}.digitaloceanspaces.com', region: 'nyc3', regions: ['nyc3', 'sfo3', 'ams3', 'fra1', 'sgp1', 'syd1'] },
  { id: 'hetzner', label: 'Hetzner Object Storage', template: 'https://{region}.your-objectstorage.com', region: 'fsn1', regions: ['fsn1', 'nbg1', 'hel1'] },
  { id: 'scaleway', label: 'Scaleway', template: 'https://s3.{region}.scw.cloud', region: 'fr-par', regions: ['fr-par', 'nl-ams', 'pl-waw'] },
  { id: 'ovh', label: 'OVHcloud', template: 'https://s3.{region}.io.cloud.ovh.net', region: 'gra', regions: ['gra', 'sbg', 'de', 'uk', 'bhs'] },
  { id: 'linode', label: 'Akamai (Linode)', template: 'https://{region}.linodeobjects.com', region: 'us-east-1', regions: ['us-east-1', 'us-southeast-1', 'eu-central-1', 'ap-south-1'] },
  { id: 'storj', label: 'Storj', template: 'https://gateway.storjshare.io', region: 'us1', pathStyle: true },
  { id: 'idrive', label: 'IDrive e2', template: 'https://{account}', region: 'us-east-1', account: 'endpoint' },
  { id: 'minio', label: 'MinIO', template: 'https://{account}', region: 'us-east-1', pathStyle: true, account: 'server' },
  { id: 'garage', label: 'Garage', template: 'https://{account}', region: 'garage', pathStyle: true, account: 'server' },
];

export function s3Endpoint(preset: S3Preset, region: string, account: string) {
  const host = account.trim().replace(/^https?:\/\//, '').replace(/\/+$/, '');
  return preset.template.replace('{region}', region.trim()).replace('{account}', host);
}

export function defaultPort(kind: string, security = 'tls') {
  switch (kind) {
    case 'ftp':
      return 21;
    case 'smb':
      return 445;
    case 'imap':
      return security === 'tls' ? 993 : 143;
    default:
      return 22;
  }
}
