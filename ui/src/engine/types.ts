// The views wmpass-session sends. None of them can hold a secret value.

export interface Summary {
  id: string;
  kind: string;
  title: string;
  subtitle: string;
  site: string | null;
  icon: string | null;
  color: string | null;
  folder: string | null;
  tags: string[];
  pinned: boolean;
  favourite: boolean;
  hidden: boolean;
  trashed: boolean;
  has_password: boolean;
  has_otp: boolean;
  has_passkey: boolean;
  attachments: number;
  modified_ms: number;
  destroy_at_ms: number | null;
}

export interface FieldSpec {
  key: string;
  label: string;
  input: string;
  secret: boolean;
  choices: string[];
  /** Values people type often, offered as chips. */
  suggestions?: string[];
}

export interface KindSpec {
  kind: string;
  fields: FieldSpec[];
}

export interface FieldView extends FieldSpec {
  has_value: boolean;
  value: string | number | boolean | null;
}

export interface CustomFieldView {
  index: number;
  name: string;
  kind: string;
  has_value: boolean;
  value: string | null;
}

export interface AttachmentView {
  blob: string;
  name: string;
  mime: string;
  size: number;
}

export interface PasskeyView {
  credential_id: string;
  rp_id: string;
  rp_name: string;
  user_name: string;
  created_ms: number;
}

export interface OtpView {
  issuer: string;
  account: string;
  kind: string;
  digits: number;
  period: number | null;
}

export interface RevisionView {
  index: number;
  field: string;
  at_ms: number;
}

export interface RemindersView {
  change_every_days: number | null;
  expires_ms: number | null;
  warn_days: number;
}

export interface Detail extends Summary {
  created_ms: number;
  fields: FieldView[];
  custom_fields: CustomFieldView[];
  has_notes: boolean;
  sites: Record<string, any>[];
  otp: OtpView | null;
  passkeys: PasskeyView[];
  attachment_list: AttachmentView[];
  links: string[];
  reminders: RemindersView;
  history: RevisionView[];
  alias: string | null;
  questions: string[];
  /** Where the site lets you change the password, for a login saved for a website. */
  change_password?: string | null;
}

export interface OtpRow {
  id: string;
  title: string;
  site: string | null;
  icon: string | null;
  otp: OtpView;
  code: string | null;
  next_code: string | null;
  seconds_left: number | null;
}

export interface FolderView {
  id: string;
  name: string;
  parent: string | null;
}

export interface HealthView {
  weak: string[];
  reused: string[][];
  old: string[];
  breached: [string, number][];
  missing_two_factor: string[];
  insecure_url: string[];
  duplicates: string[][];
  /** Logins that share a password and look like one account saved twice. */
  similar?: string[][];
  expiring: string[];
  score: number;
}

export interface StrengthView {
  score: number;
  crack_slow_hash: string;
  crack_fast_hash: string;
  warning: string | null;
  suggestions?: string[];
}

export interface CompareView {
  first: StrengthView;
  second: StrengthView;
  winner: 'first' | 'second' | 'even';
  factor: number;
}

export interface MatchRow {
  entry: Summary;
  level: 'unverified' | 'domain' | 'exact';
}

export interface SlotView {
  index: number;
  label: string;
  /** False for a duress password, absent from older engines. */
  opens?: boolean | null;
  /** One of two passwords changed from one on two devices at once. */
  conflict?: boolean;
  /** When it was last written, and on which device, if that device gave its name. */
  changed_ms?: number;
  changed_on?: string | null;
}

/** A clock that looks wrong, as the vault judges it. */
export interface ClockWarningView {
  this_device: boolean;
  ahead_device: string | null;
  by_ms: number;
  hours: number;
  message: string;
}

/** A password changed on two devices at once. `both_open` is false when only the later one stayed. */
export interface PasswordConflictView {
  slots: number[];
  both_open: boolean;
  message: string;
}

export interface VaultNotices {
  clock_warning: ClockWarningView | null;
  password_conflict: PasswordConflictView | null;
}

export interface VaultInfo {
  name: string;
  generation: number;
  read_only: boolean;
  dirty: boolean;
  shelf_open: boolean;
  entries: number;
  trash_days: number;
  history_limit: number;
  clock_warning?: ClockWarningView | null;
  password_conflict?: PasswordConflictView | null;
  kdf: { m_kib: number; t: number; p: number };
}

export interface ImportPreview {
  format: string;
  count: number;
  titles: string[];
  skipped: string[];
}

export interface PresetView {
  name: string;
  recipe: any;
}

export interface LinkedView {
  file: string;
  name: string;
  shared: boolean;
  owner: boolean;
  pending: boolean;
}

export interface NeededFactor {
  kind: 'key_file' | 'fido2' | 'webauthn' | 'yubikey';
  credential_id?: string;
  salt?: string;
  challenge?: string;
  slot?: number;
}

export type LockState =
  | { kind: 'none' }
  | { kind: 'locked' }
  | { kind: 'unlocked'; name: string; file: string; readOnly: boolean };

export type UnlockResult =
  | { kind: 'opened' }
  | { kind: 'wrong'; retryAtMs: number }
  | { kind: 'waiting'; retryAtMs: number }
  | { kind: 'needs_factor'; factors: NeededFactor[] }
  | { kind: 'hard_locked' };

export interface RelatedApp {
  package: string;
  name: string;
}

/** What goes with a login: websites, apps and, for one without a name yet, a name. */
export interface RelatedView {
  sites: string[];
  apps: RelatedApp[];
  name: string | null;
}
