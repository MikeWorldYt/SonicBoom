import type { NormalizationLanguage, NormalizationMode, NormalizeRequest, SpeechPronunciation } from '../../services/contracts';

/** Delay between a user action and the preview request it triggers. */
export const PREVIEW_DEBOUNCE_MS = 500;

const isConcreteLocale = (value: string | undefined): value is string => !!value && value.trim() !== '' && value.trim().toLowerCase() !== 'auto';

/**
 * Query fields for the synthesis endpoints. Absent or "auto" values are omitted,
 * so a legacy caller produces exactly the URL it produced before.
 */
export function pronunciationQuery(pronunciation?: SpeechPronunciation): Record<string, string> {
  const query: Record<string, string> = {};
  if (!pronunciation) return query;
  if (pronunciation.normalization) query.normalization = pronunciation.normalization;
  if (isConcreteLocale(pronunciation.locale)) query.locale = pronunciation.locale.trim();
  return query;
}

/** Locales the server offers for a speech language; empty when the language is unknown to it. */
export function localeChoices(entry: NormalizationLanguage | undefined): readonly string[] {
  return entry?.locales ?? [];
}

/** The locale selector matters only when the mode is automatic and there is a real choice. */
export function showLocaleChoice(entry: NormalizationLanguage | undefined, mode: NormalizationMode): boolean {
  return mode === 'auto' && localeChoices(entry).length > 1;
}

/**
 * The explicit locale to send. Only a tag the server listed for this language
 * qualifies; anything else (including the empty "language default" choice)
 * yields undefined so the server applies its documented default.
 */
export function effectiveLocale(entry: NormalizationLanguage | undefined, requested: string, mode: NormalizationMode): string | undefined {
  if (!showLocaleChoice(entry, mode)) return undefined;
  return localeChoices(entry).includes(requested) ? requested : undefined;
}

/** Body of POST /api/tts/normalize. Optional members are left out when unset. */
export function previewRequestBody(request: NormalizeRequest): NormalizeRequest {
  return {
    text: request.text, language: request.language,
    ...(isConcreteLocale(request.locale) ? { locale: request.locale.trim() } : {}),
    ...(request.mode ? { mode: request.mode } : {}),
    ...(request.model ? { model: request.model } : {}),
  };
}

/** Identity of a preview: two requests with equal keys describe the same spoken result. */
export function previewKey(request: NormalizeRequest): string {
  return JSON.stringify([request.text, request.language, request.locale ?? '', request.mode ?? '', request.model ?? '']);
}

/** True when a finished preview no longer matches the current form. */
export function isPreviewStale(previewedKey: string | null, currentKey: string): boolean {
  return previewedKey !== null && previewedKey !== currentKey;
}

/** Task run by `createLatestRunner`; `isCurrent()` turns false once a newer request or a cancel supersedes it. */
export type LatestTask = (signal: AbortSignal, isCurrent: () => boolean) => Promise<void>;

/**
 * Debounces user-triggered work and cancels whatever it superseded: scheduling
 * replaces a waiting task, aborts an in-flight one and invalidates its result.
 */
export function createLatestRunner(delayMs = PREVIEW_DEBOUNCE_MS) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let controller: AbortController | null = null;
  let generation = 0;
  function cancel() {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
    controller?.abort();
    controller = null;
    generation++;
  }
  function schedule(task: LatestTask) {
    cancel();
    const mine = generation;
    timer = setTimeout(() => {
      timer = undefined;
      const own = new AbortController();
      controller = own;
      // Tasks report their own failures; this only guards against an unhandled rejection.
      void task(own.signal, () => mine === generation && !own.signal.aborted).catch(() => {});
    }, delayMs);
  }
  return { schedule, cancel };
}

/** English catalog keys for change kinds the server may report. Unknown kinds fall back to the raw value. */
export const CHANGE_KIND_LABELS: Readonly<Record<string, string>> = {
  number: 'Number', decimal: 'Decimal', ordinal: 'Ordinal', percentage: 'Percentage', currency: 'Currency',
  date: 'Date', time: 'Time', unit: 'Unit', abbreviation: 'Abbreviation', range: 'Range',
};

/** Short plain-language title and explanation for each warning code. */
export const WARNING_LABELS: Readonly<Record<string, { title: string; explanation: string }>> = {
  ambiguous_number: { title: 'Ambiguous number', explanation: 'This number can be read in more than one way, so it was left as written.' },
  ambiguous_date: { title: 'Ambiguous date', explanation: 'The day and month can be read in either order, so the date was left as written. Choose a regional pronunciation to resolve it.' },
  invalid_date: { title: 'Invalid date', explanation: 'This is not a real calendar date, so it was left as written.' },
  ambiguous_currency: { title: 'Ambiguous currency', explanation: 'The currency is unclear, so the amount was left as written.' },
  ambiguous_time: { title: 'Ambiguous time', explanation: 'This could be a time or a ratio, so it was left as written.' },
  ambiguous_fraction: { title: 'Ambiguous fraction', explanation: 'This could be a fraction, a date or a ratio, so it was left as written.' },
  number_too_large: { title: 'Number too large', explanation: 'This number is too large to read out reliably, so it was left as written.' },
};
