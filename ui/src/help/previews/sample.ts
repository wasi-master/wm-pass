// Made-up items for the guide's previews. Nothing here comes from a vault.

export interface SampleRow {
  id: string;
  title: string;
  site: string | null;
  kind: string;
  subtitle: string;
  favourite: boolean;
  has_otp: boolean;
  has_password: boolean;
  attachments: number;
  color: string | null;
  icon: string | null;
}

export const SAMPLE_ROWS: SampleRow[] = [
  { id: 'a', title: 'GitHub', site: 'github.com', kind: 'login', subtitle: 'sam@example.com', favourite: true, has_otp: true, has_password: true, attachments: 0, color: null, icon: null },
  { id: 'b', title: 'GitHub', site: 'github.com', kind: 'login', subtitle: 'sam-work', favourite: false, has_otp: false, has_password: true, attachments: 0, color: null, icon: null },
  { id: 'c', title: 'Netflix', site: 'netflix.com', kind: 'login', subtitle: 'sam@example.com', favourite: false, has_otp: false, has_password: true, attachments: 0, color: null, icon: null },
  { id: 'd', title: 'Home Wi-Fi', site: null, kind: 'wifi', subtitle: 'Garden-5G', favourite: false, has_otp: false, has_password: true, attachments: 1, color: null, icon: null },
];
