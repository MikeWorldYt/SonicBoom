# Text normalization (`tn-v1`)

Normalization defaults to `auto`, including for existing clients that omit the
new option. This can change spoken output compared with earlier releases. Set
`[tts.normalization] mode = "off"` or send `normalization=off` to retain the
original text. Setting `enabled = false` disables normalization globally, even
when a request explicitly asks for `auto`. The configured `default_locale`
applies only to its matching language; other languages use their own defaults.

SonicBoom turns written forms such as dates, prices, times and units into
speakable text before synthesis. The normalizer lives in
`src/tts/normalization/`. It is Rust-native, offline and deterministic. It has
no new dependencies and makes no network calls.

Normalization is not a fix for model pronunciation. It only rewrites symbolic
text. Nothing here has been verified acoustically (see [Limitations](#limitations)).

## Modes

| Mode   | Behavior |
|--------|----------|
| `auto` | Conservative, high-confidence transforms for the features the language supports. Everything else is preserved byte-for-byte. Ambiguous input produces a warning. |
| `off`  | The spoken text is byte-identical to the input. Providers keep their own preprocessing. |

`strict` is not implemented in v1.

## Architecture

```text
original text (immutable)
  -> input length + language + locale validation   options.rs
  -> whitespace fields, protected regions          lexer.rs
  -> classification + ambiguity policy             classify.rs
  -> locale-aware spoken forms                     rules/{es,en,...}.rs
  -> splice + budget checks                        render.rs
```

- `mod.rs` holds the public API: `normalize`, `SpeechText`, `NormalizeOptions::resolve`,
  `language_coverage`, `RULESET_VERSION = "tn-v1"`, and the change, warning and error types.
- `options.rs` holds the locale table, BCP-47 canonicalization, defaults and limits.
- `lexer.rs` splits text into whitespace-delimited fields. Each field has a *core*,
  which is the field without its leading opening punctuation (`( ¿ ¡ "`) and its
  trailing closing or sentence punctuation (`. , ; : ! ? ) "`). Protected regions
  are backtick code and `<tags>`. They may span whitespace and are never touched.
  All offsets are UTF-8 byte offsets on char boundaries.
- `classify.rs` matches each core against strict patterns. A core is rewritten only
  when the whole core matches. Precedence:
  1. protected regions and identifier shapes;
  2. phone-like runs of digit groups;
  3. currency, percentages, ordinals, units, dates, times, ranges, and fractions/ratios
     (fractions and ratios produce a warning only);
  4. plain cardinals and decimals.
- `rules/*.rs` hold one `LanguageRules` implementation per language. Every renderer
  returns `Option<String>`, and `None` means "preserve".
- `render.rs` splices the replacements. Bytes outside replaced spans are copied
  unchanged. It then checks `max_output_chars` and the expansion ratio.
- `segments.rs` finds protected sentence boundaries and does grapheme-safe hard
  splitting. The Supertonic chunker in `src/tts/text.rs` uses it.
- `dictionary.rs` is a pronunciation-exception table: whole-word, case-sensitive and
  bounded. It is **not wired in** (see Limitations).

`normalize` never panics. It runs in linear time. Input above `max_input_chars` is
rejected before it is scanned.

### Budgets

- `max_input_chars`: characters of the original text. Exceeding it returns `InputTooLong`.
- `max_output_chars`: characters of the spoken text. Exceeding it returns `OutputTooLong`.
- `max_expansion_ratio`: the spoken text may have at most
  `max(input chars, 32) * ratio` characters. Exceeding it returns `ExpansionTooLarge`.
  The 32-character floor keeps short valid inputs from being rejected: `77%`
  (3 chars) becomes `setenta y siete por ciento` (26 chars), which is within the
  32 × 8 = 256 limit.

Nothing is ever truncated. Long inputs that expand heavily are still rejected.
Long digit runs are never expanded: they are preserved with `number_too_large`, so
they pass the budget unchanged. Input above `max_input_chars` is rejected.

## Languages and locales

| Language | Locales | Default | Rules |
|---|---|---|---|
| `es` | es-ES, es-MX, es-PE | es-ES | partial (see table below) |
| `en` | en-US, en-GB | en-US | partial (see table below) |
| `pt` | pt-BR, pt-PT | pt-BR | partial (see the multilingual table) |
| `fr` | fr-FR | fr-FR | partial |
| `de` | de-DE | de-DE | partial |
| `it` | it-IT | it-IT | partial |
| `ru` | ru-RU | ru-RU | partial |
| `zh` | zh-CN, zh-TW | zh-CN | partial |
| `ja` | ja-JP | ja-JP | partial |
| `ko` | ko-KR | ko-KR | partial |

Locale resolution rules:

- No locale, `""` or `auto` selects the documented default. This locale is *not
  explicit*.
- The default is never inferred from the server location or the UI language.
- A concrete tag is canonicalized (`es-pe` becomes `es-PE`). It must belong to the
  speech language, or the error is `LocaleLanguageMismatch`. It must be in the table,
  or the error is `UnsupportedLocale` (for example `es-AR`).
- Malformed tags return `InvalidLocale` (for example `es_PE`, `es-`, `123`).
- A well-formed speech language without rules (for example `nl`) passes through
  unchanged. Its locale is the language code itself.

### Feature coverage (es / en)

| Feature | es | en | Notes |
|---|---|---|---|
| cardinal_numbers | full | full | Exact integers up to 999 999 999 999 999. Larger values are preserved and produce `number_too_large`. Leading zeros (`007`) are preserved and produce `ambiguous_number`. |
| decimals | partial | partial | Every fractional digit is kept, up to 12 digits. en reads digit by digit (`3.140` → "three point one four zero"). es reads one digit, or two digits without a leading zero, as a number (`12,5` → "doce coma cinco", `1.234,56` → "… coma cincuenta y seis"). Otherwise es reads digit by digit (`3,05` → "tres coma cero cinco"). Scientific notation is not supported. |
| ordinals | partial | partial | en: `1st`, `2nd`, `21st`, … The suffix is validated, so `1th` is preserved. es: `1º`/`1ª`/`1.º` from 1 to 100, with apocope before a noun (`3º piso` → "tercer piso"). |
| percentages | full | full | `12,5%`, `12,5 %`, `-3%`. |
| currency | partial | partial | `S/` and `S/.` (PEN), `€`, `£`, `US$`, `R$`, and the codes PEN, EUR, GBP, USD, BRL, JPY. A bare `$` means USD only under en-US. Elsewhere it produces `ambiguous_currency`, and so does `¥`. Amounts must be integers or have exactly two decimals. The renderer handles singular and plural, Spanish gender (`una libra esterlina`), apocope (`veintiún soles`) and `de` after exact millions. An amount followed by a scale word (`$1.5 million`) is preserved. |
| dates | partial | partial | Slash `D/M/YYYY` and ISO `YYYY-MM-DD`, validated including leap years. Textual months and weekdays are not handled. |
| times | partial | partial | `H:MM` with hours 0–23. en also accepts AM/PM. es with AM/PM, invalid ranges, and `H:MM:SS` are preserved and produce `ambiguous_time`. |
| units | partial | partial | kg, g, km, m, cm, mm, L, mL, °C, °F, km/h, m/s, GB, MHz, ms. Case-sensitive. The unit must follow the number directly or after one space. en-GB uses `-re` spellings. |
| abbreviations | partial | partial | es: Dr., Dra., Sr., Sra., Srta. before a capitalized word; Av. before a capitalized word or a number; etc. only when not at a sentence end. en: Dr., Mr., etc. with the same rule. Ambiguous forms such as `St.` are preserved. |
| ranges | partial | partial | `10–15` (en dash, plain integers, ascending order), optionally followed by a unit. es reads "diez a quince", en reads "ten to fifteen". Hyphenated digits are preserved as IDs. |
| fractions | none | none | `½`, `3/4`, `1:2` are always preserved and produce `ambiguous_fraction`. |

Overall coverage is `partial` for every language with rules.
`language_coverage()` and `NormalizedText::coverage` report coverage per feature.

### Feature coverage (pt / fr / de / it / ru / zh / ja / ko)

| Feature | pt | fr | de | it | ru | zh | ja | ko |
|---|---|---|---|---|---|---|---|---|
| cardinal_numbers | full | partial | none | full | partial | partial | partial | partial |
| decimals | partial | partial | partial | partial | partial | partial | partial | partial |
| ordinals | partial | none | none | partial | none | none | none | none |
| percentages | full | partial | partial | full | partial | partial | partial | partial |
| currency | partial | partial | partial | partial | partial | partial | partial | partial |
| dates | partial | partial | partial | partial | none | partial | partial | partial |
| times | partial | partial | none | partial | none | partial | partial | partial |
| units | partial | partial | partial | partial | partial | partial | partial | partial |
| abbreviations | partial | partial | partial | partial | none | none | none | none |
| ranges | partial | partial | partial | none | none | none | none | none |
| fractions | none | none | none | none | none | none | none | none |

Coverage is reported per language, not per locale. Where a locale converts
less (zh-TW), the notes say so.

#### Per-language notes (agent-chosen, not yet reviewed by a fluent speaker)

Every feature below was written and tested by an agent. None has been
reviewed by a native speaker. The golden rows are `agent-unreviewed`.

- **pt** (pt-BR, pt-PT): decimal comma, dot grouping; `1.234`/`3.14`
  ambiguous as in es. Connector `e` (`mil duzentos e trinta e quatro`,
  `dois mil e vinte e seis`). Locale differences: `dezesseis`/`dezasseis`
  (and 17, 19), `bilhão` (pt-BR, 10^9) vs `mil milhões`/`bilião` (pt-PT),
  `quilômetro`/`quilómetro`, euro cents `centavos`/`cêntimos`, day 1
  `primeiro` (pt-BR) vs `um` (pt-PT). Currency: BRL (`doze reais e
  cinquenta centavos`), EUR, USD, GBP (whole amounts, `duas libras
  esterlinas`), JPY (`ienes`). PEN is preserved. Times `quinze e trinta`,
  `uma hora`. Hour 0 is preserved. `1º`/`1ª` up to 100. Gender: a number
  before an unknown noun is preserved when it ends in 1 or 2 or has
  hundreds ≥ 200 (`2 casas`). Units with a decimal below 2 (`1,5 L`) are
  preserved (singular or plural is a style choice). pt-PT groups thousands
  with spaces, so a decimal with a three-digit integer part (`234,50`) is
  preserved: it may be the tail of `1 234,50`.
- **fr** (fr-FR): decimal comma. Space grouping is split by the lexer.
  `1 234` is protected as a digit run. In `1 234,50 €` the first group
  gets `ambiguous_number` and `234,50 €` is preserved (so the spec row
  stays unchanged). Dot grouping is not recognized. Traditional
  hyphenation: `vingt et un`, `soixante et onze`, `quatre-vingts`,
  `quatre-vingt-un`, `deux cents`, `deux cent mille`, `quatre-vingts
  millions`. Nouns are plural from 2 (`1,5 kilomètre`, `zéro euro`).
  Currency: `douze euros et cinquante centimes`, `un million d'euros`,
  `vingt et une livres sterling` (whole amounts). Dates `vingt-six octobre
  deux mille vingt-six`, `premier mai`. Times: `quinze heures trente`,
  `une heure`, `zéro heure quinze`. `15h30` is preserved. `15 h` is
  preserved with `ambiguous_number`. `15 h 30` currently reads
  `15 h trente`: the minutes cannot see the preceding `h`, so this is a
  known gap that needs a previous-word or `HH h MM` classifier hook. The
  same applies to pt and it. Ordinals
  (`1er`, `2e`) are not recognized. A number ending in "un" before a noun
  (`21 maisons`) is preserved (un/une).
- **de** (de-DE): decimal comma, dot grouping. Compounds below one million
  are one word (`zweitausendsechsundzwanzig`, `hunderteins`); `eine
  Million`. Standalone integers are **not** converted: `am 3. Oktober`
  reaches the rules as `3`, and the rules cannot tell an ordinal from a
  cardinal at a sentence end. Integers in money, `%`, units and ranges are
  read, and so are decimals (`drei Komma eins vier`). Times are preserved
  with `ambiguous_time`, because `15:30 Uhr` would read "… Uhr dreißig
  Uhr". Dot dates (`26.10.2026`) are preserved (no classifier hook). ISO
  and slash dates use the dative ordinal `am sechsundzwanzigsten Oktober
  zweitausendsechsundzwanzig`. Years 1100–1999 are read in hundreds
  (`neunzehnhundertneunundneunzig`). Currency: `zwölf Euro und fünfzig
  Cent`, `ein Euro`, `fünf Pfund` (whole amounts).
- **it** (it-IT): decimal comma, dot grouping; `15.30` is ambiguous.
  `ventuno`, `ventotto`, `ventitré`, `centotré`, `centottanta`,
  `duemilaventisei`, `un milione`, `mille miliardi` (10^12). Currency:
  `dodici euro e cinquanta centesimi`, `un euro`, `due milioni di euro`,
  `una sterlina` (whole amounts). Dates `primo maggio duemilaventisei`.
  Times `quindici e trenta`, `una`. Hour 0 is preserved. Ordinals 1–100
  (`terzo`, `undicesimo`, `ventitreesimo`). A number ending in 1 before a
  noun is preserved (un/uno/una).
- **ru** (ru-RU): decimal comma; space grouping is handled as in fr. Only
  the nominative/accusative reading is produced. A number before a word is
  read only when the word is a verified noun (рубль, доллар, евро,
  процент, год below 1000, километр, килограмм) in the form the number
  requires (`1 рубль`, `2 рубля`, `5 рублей`). Other nouns, other case
  forms (`5 рублями`) and ordinal years (`в 2021 году`) are preserved with
  `ambiguous_number`. Decimals use `целых`/`десятых`/`сотых`/`тысячных` (up
  to 3 digits). Currency: `двенадцать долларов пятьдесят центов`,
  `двенадцать евро пятьдесят центов`, `две иены`. Units: km and kg only.
  The Latin forms are needed: `км` is not a recognized unit. Known limit: a
  genitive context (`до 5 рублей`) still gets `пять`. Dates, times,
  ordinals and ranges are preserved.
- **zh** (zh-CN, zh-TW): decimal point, comma grouping. `十`, `十一`,
  `一百一十`, `一千零一`, `十万零一十`, `一亿零一`, `万亿`. `二` is used in
  cardinals (`二千零二十六`). `两` is used for two o'clock (`两点`) and an
  exact two before a currency or measure (`两公里`). Year digits are read one
  by one: `二零二六年十月二十六日`. zh-TW uses Traditional characters (`萬`,
  `億`, `點`, `兩`) and preserves currency and units (Taiwan usage not
  verified). Numbers attached to Hanzi (`2026年10月26日`, `3个`) are not
  whitespace-delimited and stay unchanged. A number followed by a Hanzi
  word is preserved (measure words).
- **ja** (ja-JP): kanji cardinals without zero placeholders (`二千二十六`,
  `千一`, `一万`, `一千万`, `千二百万`). Year cardinal:
  `二千二十六年十月二十六日`. Times `十五時三十分`; decimals `三点一四`;
  `パーセント`. `500 JPY` → `五百円`. `¥` is ambiguous. Amounts with cents
  are preserved. Counter readings are not decided: a number followed by a
  kanji/kana word (`3 人`) is preserved, and attached forms (`500円`, `3人`)
  stay unchanged.
- **ko** (ko-KR): Sino-Korean cardinals (`십`, `만`, `일억`) with a space
  after each 만/억/조 section (`만 이천`). Dates `이천이십육 년 시월 이십육
  일` with `유월`/`시월`. Clock hours 1–12 use native numerals (`세 시 삼십
  분`). Hours 0 and 13–23 are preserved with `ambiguous_time`. A number
  before a Hangul word is read only for Sino-Korean nouns (원, 년, 일, 초,
  층, 퍼센트, metric units): `10000 원` → `만 원`. Native counters (`3 개`)
  and `6 월` (would need `유월`) are preserved. Attached forms (`2026년`)
  stay unchanged.

Numeric slash dates are not a zh/ja/ko convention. When both readings are
valid under an explicit locale, those languages use month/day. The
framework cannot disable that conversion per locale.

#### Chosen wordings (agent-chosen, not yet reviewed by a fluent speaker)

- es time: hour-minute reading, the same style as the spec's `15:30` → "quince
  treinta". So `09:05` → "nueve cinco" (no "cero"), `21:21` → "veintiuna veintiuno",
  and on the hour `21:00` → "veintiuna en punto". Hours use the feminine form
  (`1:30` → "una treinta").
- en time: `15:30` → "fifteen thirty", `9:05` → "nine oh five", `3:00` → "three o'clock",
  `15:00` → "fifteen hundred", `3:30 PM` → "three thirty P M".
- en years in dates: `2026` → "twenty twenty-six", `2005` → "two thousand five",
  `1905` → "nineteen oh five". A plain integer `2026` outside a date is a cardinal:
  "two thousand twenty-six".
- es dates: day 1 → "primero". The year is a cardinal: "dos mil veintiséis".
- en-GB adds "and" after hundreds: "one hundred and five".

## Ambiguity policy

Ambiguous input is preserved byte-for-byte and reported. It is never guessed.

| Input | es | en |
|---|---|---|
| `1.234` | `ambiguous_number` | decimal ("one point two three four") |
| `1,234` | `ambiguous_number` | integer ("one thousand two hundred thirty-four") |
| `3.14` | `ambiguous_number` (dot decimal) | decimal |
| `3,14` | decimal | `ambiguous_number` |
| `1.234,56` | decimal | `ambiguous_number` |
| `S/ 1.500` | in money a 3-digit group is grouping: "mil quinientos soles" | — |
| `21 casas`, `200 personas` | `ambiguous_number`: the gender of the noun is unknown | — |
| `01/02/2026` | converted only under an explicit locale (es: day/month); with the default locale, `ambiguous_date` | converted only under an explicit locale (en-US: month/day; en-GB: day/month) |
| `31/12/2026` | only one valid reading, so it is converted | converted, rendered in the locale style |
| `31/02/2026`, `2026-02-30` | `invalid_date` | `invalid_date` |
| `1/2/26` (2-digit year) | `ambiguous_date` | `ambiguous_date` |
| `$5` | `ambiguous_currency` | USD under en-US; `ambiguous_currency` under en-GB |

Note on es-MX: everyday Mexican writing often uses a decimal point. Every Spanish
locale reads the decimal comma, so es-MX `3.14` is preserved with a warning rather
than misread.

Protected and never rewritten:

- URLs and emails
- @handles and #tags
- paths and `key=value` text
- IPv4, `IP:port` and IPv6 (`::`)
- versions (`1.21.11`, `v1.2.3-beta.2`)
- UUIDs and hex hashes
- alphanumeric codes (`Qwen3`, `4K`, `3D`, `COVID-19`)
- hyphenated digit IDs (`123-456`)
- phone-like digit groups (`987 654 321`, `+51 …`)
- backtick code and `<tags>`

Warnings use the snake_case codes `ambiguous_number`, `ambiguous_date`,
`invalid_date`, `ambiguous_currency`, `ambiguous_time`, `ambiguous_fraction` and
`number_too_large`. Byte offsets refer to the original text. When
`report_ambiguities` is false, no warnings are returned.

## Sentence segmentation (Supertonic)

`TextProcessor::split_sentences_with_limit` uses `segments::sentence_spans`:

- `.`, `?`, `!`, `;` and `…` end a sentence only when the terminator run, plus any
  closing quotes or brackets, is followed by whitespace or the end of the text.
- A single `.` after a known abbreviation (Dr, Sr, Av, Mr, …) or a single capital
  initial (`J.`) does not end a sentence.
- `。 ？ ！ ；` and newlines always end a sentence.
- Terminators inside backtick code or `<tags>` are ignored.

As a result, `3.14`, `10.0.0.1:8080`, `https://example.org/a?b=3.14` and `v1.2.3`
are never cut. An oversized piece is split at whitespace first. The last resort is a
hard split between grapheme clusters: combining marks, ZWJ sequences, variation
selectors, skin tones and flags stay whole. A single cluster longer than the limit
is split by characters. No chunk exceeds `max_chars`.

`TextProcessor::encode_with_report` returns the token ids, the mask, and the
number of characters dropped by the unicode indexer. It returns the count only,
never the characters. `encode` returns the same ids and mask as before.

## Tests

- Unit tests in `src/tts/normalization/**`: the number parser, the renderers for
  each language, lexer, segments, budgets, option resolution, and malformed locales.
- A deterministic fuzz test: 6 000 random inputs from a seeded xorshift. It checks
  for no panic, the expansion bound, ordered char-boundary spans, preserved bytes
  outside changes, `off` identity, and idempotence for outputs without digits.
  There is also a test with adversarial long inputs.
- Golden corpus: `tests/fixtures/tts_normalization/*.jsonl`, run by
  `cargo test --test tts_normalization --no-default-features`.
  - Each case has: id, language, locale (`auto` means the default), mode, input,
    expected_spoken or expected_unchanged, warning_codes, features, reviewed_by.
  - `reviewed_by: "spec"` marks rows copied verbatim from the specification.
  - `"agent-unreviewed"` marks expectations written by the implementing agent. They
    still need review by a fluent speaker; they are not human-reviewed.
  - There is one fixture file per language with rules (`es`, `en`, `pt`, `fr`,
    `de`, `it`, `ru`, `zh`, `ja`, `ko`), plus `negative.jsonl` for protected,
    ambiguous and invalid inputs.
  - `passthrough.jsonl` holds the case for a well-formed language without rules
    (`nl`): text with a number, a date and a currency word stays byte-identical
    with no warnings. The former pt/fr/de/it/ru/zh/ja/ko pass-through rows were
    removed when those languages got rules; their per-language files now cover
    them.

## Limitations

- **No acoustic verification.** No paired raw/normalized audio has been produced or
  listened to.
- **No dictionary API.** `dictionary.rs` is not wired into `normalize`, ships no
  entries, and has no storage or authorization. `ChangeKind` has no dictionary
  variant yet.
- **Pocket splitter untouched.** Only the text that Pocket receives changes. Pocket
  parity with real ONNX models is a follow-up.
- **Indexer NFKD open item.** `TextProcessor::encode` still applies NFKD before the
  unicode indexer. NFKD decomposes `ñ`/`á` into a base letter plus a combining mark.
  Whether the Supertonic indexer maps those marks needs checking against the real
  `unicode_indexer.json`. `encode_with_report` exposes the dropped count for that
  check.
- pt, fr, de, it, ru, zh, ja and ko have partial rules (see the
  multilingual table). Their expectations have not been reviewed by fluent
  speakers.
- Spanish numbers before an unknown noun are preserved when the reading depends on
  gender.
- Not handled: textual dates, weekdays, durations, UTC offsets, scientific notation,
  negative money, and `10 – 15` with spaces.

## Adding a language

1. In `rules/<lang>.rs`, return `Conventions` from `conventions(locale)` for the
   supported locales. While it returns `None`, the classifier never runs.
2. Override the renderers for the features you implement. Return `None` for anything
   you cannot render with certainty.
3. Report honest `coverage` for each feature.
4. Add a `tests/fixtures/tts_normalization/<lang>.jsonl` file with at least 15
   reviewed cases, including negative ones. Mark who reviewed each case.
5. If the language needs new locales, add them to `options.rs::LOCALE_TABLE`.
