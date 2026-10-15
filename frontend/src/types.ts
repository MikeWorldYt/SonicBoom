export interface ProviderCardData {
  status: { provider_id: string; state: string; effective_device?: string; active_model?: string; detail?: string; enabled: boolean };
  models: string;
}
export interface HomeData { page: 'home'; modelReady: boolean; voices: string[]; status: string; providers: ProviderCardData[] }
export interface AdminToken { id: string; fingerprint: string; created: string; expires: string; status: string; revoked: boolean }
export interface AdminData { page: 'admin' | 'login' | 'error'; error?: string; csrf?: string; newToken?: string | null; tokens?: AdminToken[] }
export type PageData = HomeData | AdminData | { page: 'setup' };
