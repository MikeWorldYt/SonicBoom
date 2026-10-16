import type { JsonObject } from './contracts';

type ConfigApi = {
  config(): Promise<{ revision: number }>;
  patchConfig(changes: JsonObject, revision?: number): Promise<unknown>;
};

/** Select and enable one verified Pocket bundle in a single config transaction. */
export async function activatePocketModel(api: ConfigApi, model: string, supported: string[]) {
  if (!supported.includes(model)) throw new Error('Pick a supported Pocket model.');
  const changes = { providers: { pocket_tts: { model_id: model, enabled: true } } };
  const snapshot = await api.config();
  try { await api.patchConfig(changes, snapshot.revision); }
  catch (error) {
    if (!(error instanceof Error) || !('status' in error) || error.status !== 409) throw error;
    // Another settings writer may have changed the revision. Retry the same
    // explicit selection once, preserving all unrelated settings.
    const latest = await api.config();
    await api.patchConfig(changes, latest.revision);
  }
}
