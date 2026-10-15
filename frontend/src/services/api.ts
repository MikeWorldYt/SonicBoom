import type { HttpClient } from './http';
import { jsonBody } from './http';
import type { ServerInfo, ProvidersResponse, BuiltinVoice, Voice, ConfigSnapshot, ConfigStatus, Schema, SettingSchema, JsonObject, AudioDevice, Job, CloneInput, SetupStatus, SetupInput, SetupResult } from './contracts';
const id = encodeURIComponent;
export function createApi(http: HttpClient) {
  return {
    info: () => http.json<ServerInfo>('/api/info'),
    providers: () => http.json<ProvidersResponse>('/api/providers'),
    builtinVoices: async () => (await http.json<{ data: BuiltinVoice[] }>('/v1/voices')).data,
    voices: async () => (await http.json<{ data: Voice[] }>('/api/voices')).data,
    createVoice: (input: CloneInput, file: File) => {
      const params = new URLSearchParams(Object.entries(input).filter(([, value]) => value !== undefined) as [string, string][]);
      return http.json<Voice>(`/api/voices/clones?${params}`, { method: 'POST', headers: { 'Content-Type': 'audio/wav' }, body: file });
    },
    deleteVoice: (voice: string) => http.response(`/api/voices/${id(voice)}`, { method: 'DELETE' }),
    previewVoice: (voice: string, text: string) => http.blob(`/api/voices/${id(voice)}/preview`, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: text }),
    config: () => http.json<ConfigSnapshot>('/api/config'),
    configStatus: () => http.json<ConfigStatus>('/api/config/status'),
    schema: async (): Promise<Schema> => {
      const value = await http.json<Schema | { settings: SettingSchema[] }>('/api/config/schema');
      return Array.isArray(value.settings) ? Object.fromEntries(value.settings.map(setting => [setting.path, setting])) : value as Schema;
    },
    patchConfig: (changes: JsonObject, revision?: number) => http.json<unknown>('/api/config', { method: 'PATCH', ...jsonBody({ changes, ...(revision === undefined ? {} : { expected_revision: revision }) }) }),
    configAction: (action: 'reload' | 'validate' | 'open') => http.response(`/api/config/${action}`, { method: 'POST' }),
    audioDevices: async () => (await http.json<{ devices: AudioDevice[] }>('/api/audio/devices')).devices,
    audioOutput: () => http.json<{ device: string; available: boolean }>('/api/audio/output'),
    testAudio: (text: string) => http.response('/api/tts/play', { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: text }),
    createJob: (text: string, language: string, voice: string, cloned: boolean, signal?: AbortSignal) => {
      const params = new URLSearchParams({ lang: language, format: 'wav', [cloned ? 'profile' : 'voice']: voice });
      return http.json<{ id: string }>(`/api/tts/jobs?${params}`, { method: 'POST', headers: { 'Content-Type': 'text/plain' }, body: text, signal });
    },
    job: (job: string, signal?: AbortSignal) => http.json<Job>(`/api/tts/jobs/${id(job)}`, { signal }),
    jobEvents: (job: string, signal?: AbortSignal) => http.response(`/api/tts/jobs/${id(job)}/events`, { signal }),
    jobAudio: (job: string, signal?: AbortSignal) => http.blob(`/api/tts/jobs/${id(job)}/audio`, { signal }),
    cancelJob: (job: string) => http.response(`/api/tts/jobs/${id(job)}/cancel`, { method: 'POST' }),
    setupStatus: () => http.json<SetupStatus>('/setup/status'),
    setupDevices: async () => (await http.json<{ devices: AudioDevice[] }>('/setup/audio-devices')).devices,
    completeSetup: (input: SetupInput) => http.json<SetupResult>('/setup/complete', { method: 'POST', ...jsonBody(input) }),
  };
}
export type Api = ReturnType<typeof createApi>;
