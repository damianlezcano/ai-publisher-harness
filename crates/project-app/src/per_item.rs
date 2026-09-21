//! Per-item summarization over an explicit prior `MaterialSet` referent.
//!
//! This is the "resumí cada uno" execution path. It is NOT a retrieval mode and
//! never performs top-K hybrid retrieval or re-embedding: every material in the
//! referent gets exactly one compact local representative, the representatives
//! are composed into a bounded number of aggregate remote requests (remote-call
//! count is a function of the provider-input budget, never of the material
//! count), the output is validated for exact cardinality, and missing entries
//! receive an explicit localized fallback instead of silently shrinking the
//! result set.

use std::collections::BTreeMap;

use project_knowledge::{SummaryEvidenceRef, SummaryLevel, SummaryRequest, SummaryUsage};

/// Maximum materials per single remote request. 50 and 100 materials stay in
/// one aggregate call; 500 use five bounded calls.
pub const PER_ITEM_MAX_ITEMS_PER_CALL: usize = 100;
/// Default per-item word limit when the query does not request a specific one.
pub const PER_ITEM_MAX_WORDS: usize = 40;
/// Bounded excerpt characters per representative chunk.
pub const REPRESENTATIVE_CHUNK_CHARS: usize = 240;
/// Maximum deterministic representative chunks per material (beginning / middle
/// / end) when no persisted document summary exists.
pub const REPRESENTATIVE_MAX_CHUNKS: usize = 3;
/// Hard per-call application request budget, measured in UTF-8 bytes of the
/// EXACT serialized compact prompt that will be sent (instruction text, source
/// names, `[Mxxx]` keys, `(fuente: ...)` framing, separators/newlines, and
/// bounded representatives all count). Batch packing is bounded by BOTH the
/// item cap and this byte budget; a batch is split the moment adding the next
/// item would exceed it. The value preserves the historical ~24k-unit capacity
/// (one unit ≈ 3 bytes). A single bounded item always fits by construction
/// (bounded representative + bounded source name), so there is no infinite loop.
pub const PER_ITEM_MAX_REQUEST_BYTES_PER_CALL: usize = 72_000;
/// Maximum UTF-8 characters of any compact per-item representative. A reused
/// Ready document summary must not bypass this bound: it is truncated safely
/// (character boundary) to the same per-item bounded-context contract as the
/// deterministic chunk representative.
pub const PER_ITEM_REPRESENTATIVE_MAX_CHARS: usize = 800;
/// Maximum UTF-8 characters of a compact per-item source name. Together with
/// [`PER_ITEM_REPRESENTATIVE_MAX_CHARS`] this guarantees a single item can never
/// exceed the per-call byte budget (an unbounded filename would otherwise let an
/// oversized single-item request bypass the hard budget).
pub const PER_ITEM_SOURCE_NAME_MAX_CHARS: usize = 200;

/// Localized, truthful marker when the remote response omits a requested entry
/// (or the provider call fails). This indicates a SYNTHESIS failure for this
/// slot — not a lack of source content — so a partial provider output is never
/// silently presented as a fully successful turn. Exact cardinality is
/// preserved: the slot exists, the failure is explicit.
pub const PER_ITEM_SYNTHESIS_FAILED: &str =
    "No se pudo generar el resumen de este archivo en esta ejecución.";
/// Localized slot when a referent material no longer resolves to READY content.
pub const PER_ITEM_MISSING: &str = "Este archivo ya no está disponible en Knowledge.";

/// One material prepared for a per-item request.
#[derive(Clone, Debug)]
pub struct PerItemMaterial {
    /// Stable request key: `M001`, `M002`, ... Unique per request.
    pub key: String,
    pub material_id: String,
    pub source_name: String,
    /// Compact local representative (persisted summary or deterministic chunks).
    pub representative: String,
    /// Position in the original referent, so rendered slots keep exact order
    /// even when some materials are missing and others are batched.
    pub order: usize,
}

/// One rendered per-item result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PerItemOutcome {
    pub material_id: String,
    pub source_name: String,
    pub summary: String,
    /// True when the entry is an explicit fallback, not a model summary.
    pub fallback: bool,
}

/// Execution knobs for the per-item path. Production uses [`Default`]; tests
/// may force a smaller batch to prove bounded aggregate calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerItemExecutionOptions {
    pub max_items_per_call: usize,
    pub max_words_per_item: usize,
    /// Hard per-call budget in UTF-8 bytes of the exact serialized compact
    /// request (see [`serialize_summary_prompt`]).
    pub max_serialized_bytes_per_call: usize,
}

impl Default for PerItemExecutionOptions {
    fn default() -> Self {
        Self {
            max_items_per_call: PER_ITEM_MAX_ITEMS_PER_CALL,
            max_words_per_item: PER_ITEM_MAX_WORDS,
            max_serialized_bytes_per_call: PER_ITEM_MAX_REQUEST_BYTES_PER_CALL,
        }
    }
}

/// Extracts an explicit word-limit number from the query ("no más de 20
/// palabras", "20 words"), or `None` when no number sits near a word-limit noun.
pub fn extract_per_item_word_limit(query: &str) -> Option<usize> {
    let normalized = query.to_lowercase();
    let tokens: Vec<&str> = normalized
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    for (index, token) in tokens.iter().enumerate() {
        let Ok(limit) = token.parse::<usize>() else {
            continue;
        };
        let window_start = index.saturating_sub(2);
        let window = tokens
            .iter()
            .skip(window_start)
            .take(5)
            .copied()
            .collect::<Vec<_>>();
        if window.iter().any(|word| WORD_LIMIT_NOUNS.contains(word)) {
            return Some(limit);
        }
    }
    None
}

/// Resolves the per-item word limit with a deterministic default.
pub fn per_item_word_limit(query: &str, options: PerItemExecutionOptions) -> usize {
    extract_per_item_word_limit(query).unwrap_or(options.max_words_per_item)
}

/// Canonical application-level prompt serialization for a summary request. This
/// is the EXACT bytes sent to the provider (instruction + labelled evidence with
/// `[label] (fuente: source_name)` framing and separators). The compact packer's
/// budget and the executor's payload both derive from this single function, so
/// they can never diverge.
pub fn serialize_summary_prompt(
    instruction: &str,
    labels: &[SummaryEvidenceRef],
    evidence_texts: &[String],
) -> String {
    let mut body = String::new();
    body.push_str(instruction);
    body.push('\n');
    body.push_str("\nEvidencia (ÚNICAMENTE esta evidencia):\n");
    for (evidence_ref, text) in labels.iter().zip(evidence_texts) {
        push_item_framing(
            &mut body,
            &evidence_ref.label,
            &evidence_ref.source_name,
            text,
        );
    }
    body
}

/// Appends the exact framing an item contributes to a serialized prompt. Shared
/// by [`serialize_summary_prompt`] and [`per_item_serialized_bytes`] so the
/// budget arithmetic is identical to the emitted payload.
fn push_item_framing(body: &mut String, label: &str, source_name: &str, text: &str) {
    body.push_str(&format!(
        "\n[{}] (fuente: {}):\n{}\n",
        label, source_name, text
    ));
}

/// The exact UTF-8 bytes one [`PerItemMaterial`] contributes to a serialized
/// compact prompt (its `[Mxxx]` framing, source name, representative, and
/// trailing newline). Derived from the same framing helper as
/// [`serialize_summary_prompt`], so packing and payload can never diverge.
pub fn per_item_serialized_bytes(item: &PerItemMaterial) -> usize {
    let mut body = String::new();
    push_item_framing(
        &mut body,
        &item.key,
        &item.source_name,
        &item.representative,
    );
    body.len()
}

/// The exact UTF-8 bytes of the fixed per-batch prompt overhead (the
/// instruction text plus the evidence header), for the given word limit.
pub fn per_item_prompt_overhead_bytes(max_words: usize) -> usize {
    serialize_summary_prompt(&per_item_instruction(max_words), &[], &[]).len()
}

/// Bounds any compact representative (a reused Ready summary or a locally-built
/// chunk representative) to [`PER_ITEM_REPRESENTATIVE_MAX_CHARS`] UTF-8
/// characters. Truncation is always on a character boundary, so the result
/// stays valid UTF-8.
pub fn bound_representative(text: &str) -> String {
    truncate_chars(text.trim(), PER_ITEM_REPRESENTATIVE_MAX_CHARS)
}

/// Bounds a compact source name to [`PER_ITEM_SOURCE_NAME_MAX_CHARS`] UTF-8
/// characters (character boundary). Guarantees a single item can never exceed
/// the per-call byte budget even with a pathological filename.
pub fn bound_source_name(text: &str) -> String {
    truncate_chars(text.trim(), PER_ITEM_SOURCE_NAME_MAX_CHARS)
}

/// Greedily packs materials in selected order into a deterministic, bounded
/// number of batches. A batch is split when adding the next item would exceed
/// either the item cap or the hard serialized-byte budget. The budget is the
/// exact UTF-8 size of the serialized prompt [`build_per_item_request`] will
/// emit (shared [`serialize_summary_prompt`] framing), so every generated
/// request stays within the configured limit by construction. There is never a
/// per-document provider loop.
///
/// A single item is guaranteed to fit the DEFAULT budget because both its
/// representative and source name are bounded. For any injected budget smaller
/// than a single item, the representative is deterministically shortened
/// (UTF-8 char boundary) so the item still fits instead of emitting an
/// oversized request or looping forever.
pub fn pack_per_item_batches(
    items: &[PerItemMaterial],
    options: PerItemExecutionOptions,
) -> Vec<Vec<PerItemMaterial>> {
    let overhead = per_item_prompt_overhead_bytes(options.max_words_per_item);
    let mut batches: Vec<Vec<PerItemMaterial>> = Vec::new();
    let mut current: Vec<PerItemMaterial> = Vec::new();
    let mut current_bytes = overhead;
    for mut item in items.iter().cloned() {
        let mut item_bytes = per_item_serialized_bytes(&item);
        if current.is_empty() && item_bytes + overhead > options.max_serialized_bytes_per_call {
            item = truncate_item_to_fit(item, options.max_serialized_bytes_per_call, overhead);
            item_bytes = per_item_serialized_bytes(&item);
        }
        let over_item_cap = !current.is_empty() && current.len() >= options.max_items_per_call;
        let over_budget = !current.is_empty()
            && current_bytes + item_bytes > options.max_serialized_bytes_per_call;
        if over_item_cap || over_budget {
            batches.push(std::mem::take(&mut current));
            current_bytes = overhead;
        }
        current_bytes += item_bytes;
        current.push(item);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Deterministically shortens an item so it fits a per-call byte budget alone:
/// first the representative is halved on character boundaries until the item
/// fits, then it is cleared. The source name is already bounded by
/// [`bound_source_name`], so clearing the representative is sufficient in
/// practice; the item is never dropped and never emitted oversized.
fn truncate_item_to_fit(
    mut item: PerItemMaterial,
    budget: usize,
    overhead: usize,
) -> PerItemMaterial {
    while per_item_serialized_bytes(&item) + overhead > budget && !item.representative.is_empty() {
        let chars = item.representative.chars().count();
        let next = chars / 2;
        if next == 0 {
            item.representative.clear();
            break;
        }
        item.representative = truncate_chars(&item.representative, next);
    }
    item
}

/// Builds one bounded aggregate request for a batch of materials. Each material
/// carries a stable key (M001...) and its compact representative; the remote
/// model is asked to emit one summary per key. `estimated_input_units` is the
/// exact UTF-8 byte size of the serialized prompt (shared
/// [`serialize_summary_prompt`] framing), so the reported budget is the actual
/// payload size — instruction, source names, `[Mxxx]` keys, `(fuente: ...)`
/// framing, separators, and bounded representatives all count.
pub fn build_per_item_request(items: &[PerItemMaterial], max_words: usize) -> SummaryRequest {
    let instruction = per_item_instruction(max_words);
    let labels: Vec<SummaryEvidenceRef> = items
        .iter()
        .map(|item| SummaryEvidenceRef {
            label: item.key.clone(),
            source_label: item.key.clone(),
            source_name: item.source_name.clone(),
            chunk_label: item.key.clone(),
        })
        .collect();
    let evidence_texts: Vec<String> = items
        .iter()
        .map(|item| item.representative.clone())
        .collect();
    let estimated_input_units =
        serialize_summary_prompt(&instruction, &labels, &evidence_texts).len();
    SummaryRequest {
        level: SummaryLevel::Document,
        labels,
        evidence_texts,
        instruction,
        estimated_input_units,
    }
}

/// The deterministic per-item instruction. Every key is named explicitly so
/// exact-cardinality validation is possible without free-form model numbering.
pub fn per_item_instruction(max_words: usize) -> String {
    format!(
        "Para cada clave etiquetada (M001, M002, ...) redactá UN resumen breve del material indicado, de no más de {max_words} palabras. Emití EXACTAMENTE una línea por clave con este formato: 'M001: <resumen>'. No omitas ninguna clave, no inventes claves nuevas, no agregues comentarios ni texto adicional fuera de las líneas por clave. No uses markdown ni listas con viñetas."
    )
}

/// Parses the model response into a `key -> summary` map. Missing keys are
/// simply absent (the caller substitutes an explicit fallback); every returned
/// summary is truncated deterministically to `max_words`.
///
/// Parsing is line-anchored and delimiter-required, so:
/// - a key must start a line (a leading `Mxxx` token followed by a `:`/`.`/`-`/
///   `)`/`]`/`>` marker) to open a slot;
/// - an `Mxxx` token embedded inside another summary's prose never opens a slot
///   and never truncates/misattributes that summary;
/// - unknown keys (`M999`) are ignored and create no slot;
/// - a duplicated expected key is deterministic (the first occurrence wins);
/// - prose without keys creates no slot.
pub fn parse_per_item_output(
    text: &str,
    expected_keys: &[String],
    max_words: usize,
) -> BTreeMap<String, String> {
    let expected: std::collections::BTreeSet<String> =
        expected_keys.iter().map(|key| key.to_uppercase()).collect();
    let mut result = BTreeMap::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut current: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();

    for line in text.lines() {
        if let Some((key, rest)) = key_line_prefix(line) {
            if expected.contains(&key) {
                finalize_current(&mut result, &mut current, &mut current_lines, max_words);
                if seen.contains(&key) {
                    current = None;
                } else {
                    seen.insert(key.clone());
                    current = Some(key);
                    current_lines.push(rest.to_owned());
                }
            }
            // An unknown key line (M999) is ignored entirely: it opens no slot
            // and never pollutes an adjacent summary.
        } else if current.is_some() {
            current_lines.push(line.to_owned());
        }
    }
    finalize_current(&mut result, &mut current, &mut current_lines, max_words);
    result
}

fn finalize_current(
    result: &mut BTreeMap<String, String>,
    current: &mut Option<String>,
    current_lines: &mut Vec<String>,
    max_words: usize,
) {
    if let Some(key) = current.take() {
        let summary = truncate_words(current_lines.join(" ").trim(), max_words);
        if !summary.is_empty() {
            result.insert(key, summary);
        }
    }
    current_lines.clear();
}

/// Recognizes a line that opens a keyed slot: a leading `Mxxx` token (at least
/// one digit) immediately followed by a delimiter. Returns the uppercased key
/// and the content after the delimiter, or `None` when the line is prose. A
/// bare `M001` word inside prose (no delimiter) is never treated as a key.
fn key_line_prefix(line: &str) -> Option<(String, &str)> {
    let trimmed = line.trim_start();
    let key_end = trimmed.find(|c: char| !c.is_ascii_alphanumeric())?;
    if key_end == 0 {
        return None;
    }
    let key = trimmed[..key_end].to_uppercase();
    if !is_m_key(&key) {
        return None;
    }
    let after = trimmed[key_end..].trim_start();
    let mut chars = after.chars();
    let delimiter = chars.next()?;
    if !matches!(delimiter, ':' | '.' | '-' | ')' | ']' | '>') {
        return None;
    }
    let rest = after[delimiter.len_utf8()..].trim_start();
    Some((key, rest))
}

fn is_m_key(key: &str) -> bool {
    let bytes = key.as_bytes();
    bytes.len() > 1 && bytes[0] == b'M' && bytes[1..].iter().all(u8::is_ascii_digit)
}

/// Renders one entry per material as `N. <source_name>: <summary>`. The exact
/// source name is the per-item provenance; a global `Fuentes:` block is never
/// appended for this action.
pub fn render_per_item_outcomes(outcomes: &[PerItemOutcome]) -> String {
    let mut out = String::new();
    for (index, outcome) in outcomes.iter().enumerate() {
        out.push_str(&format!(
            "{}. {}: {}\n",
            index + 1,
            outcome.source_name,
            outcome.summary
        ));
    }
    out.trim_end().to_owned()
}

/// Deterministic bounded representative from persisted chunks: beginning,
/// middle, and end chunks, each truncated to [`REPRESENTATIVE_CHUNK_CHARS`].
pub fn chunk_representative(chunks: &[(String, String)]) -> String {
    if chunks.is_empty() {
        return String::new();
    }
    let picks = bounded_chunk_picks(chunks.len());
    let mut parts = Vec::new();
    for pick in picks {
        let excerpt = truncate_chars(chunks[pick].1.trim(), REPRESENTATIVE_CHUNK_CHARS);
        if !excerpt.is_empty() {
            parts.push(excerpt);
        }
    }
    parts.join(" … ")
}

fn bounded_chunk_picks(count: usize) -> Vec<usize> {
    let mut picks = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let first = 0;
    let middle = count / 2;
    let last = count - 1;
    for candidate in [first, middle, last] {
        if seen.insert(candidate) {
            picks.push(candidate);
        }
    }
    picks
}

fn truncate_words(text: &str, max_words: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= max_words {
        return words.join(" ");
    }
    words[..max_words].join(" ")
}

fn truncate_chars(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    text.chars().take(max_chars).collect()
}

const WORD_LIMIT_NOUNS: &[&str] = &["palabra", "palabras", "palabrita", "word", "words"];

/// Accumulated accounting for one per-item turn.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PerItemAccounting {
    pub remote_calls: usize,
    pub estimated_input_units: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub provider_actual: bool,
}

impl PerItemAccounting {
    pub fn add_request(&mut self, usage: &SummaryUsage) {
        let first_request = self.remote_calls == 0;
        self.remote_calls += 1;
        self.provider_actual |= usage.provider_actual;
        self.input_tokens = add_optional(self.input_tokens, usage.input_tokens, first_request);
        self.output_tokens = add_optional(self.output_tokens, usage.output_tokens, first_request);
        self.cache_read_tokens = add_optional(
            self.cache_read_tokens,
            usage.cache_read_tokens,
            first_request,
        );
        self.cache_write_tokens = add_optional(
            self.cache_write_tokens,
            usage.cache_write_tokens,
            first_request,
        );
        self.cost_usd = add_optional_f64(self.cost_usd, usage.cost_usd, first_request);
    }

    pub fn add_units(&mut self, units: usize) {
        self.estimated_input_units += units;
    }
}

fn add_optional(current: Option<u64>, next: Option<u64>, first_request: bool) -> Option<u64> {
    match (current, next) {
        (None, Some(next)) if first_request => Some(next),
        (Some(current), Some(next)) => current.checked_add(next),
        _ => None,
    }
}

fn add_optional_f64(current: Option<f64>, next: Option<f64>, first_request: bool) -> Option<f64> {
    match (current, next) {
        (None, Some(next)) if first_request => Some(next),
        (Some(current), Some(next)) => Some(current + next),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_and_summary_prompts_do_not_use_knowledge_answer_grounding() {
        let prompt = serialize_summary_prompt("Resumí cada fuente de forma compacta.", &[], &[]);
        assert!(
            !prompt.contains(project_agent::knowledge_answer_grounding_instruction()),
            "K6/PerItem scratch prompts must not use the Knowledge-answer contract"
        );
        assert!(!prompt.contains("corpus Knowledge"));
    }

    #[test]
    fn accounting_keeps_first_reported_provider_usage_and_rejects_partial_sums() {
        let reported = SummaryUsage {
            input_tokens: Some(10),
            output_tokens: Some(5),
            cache_read_tokens: Some(2),
            cache_write_tokens: Some(1),
            cost_usd: Some(0.01),
            provider_actual: true,
        };
        let unavailable = SummaryUsage::default();
        let mut accounting = PerItemAccounting::default();
        accounting.add_request(&reported);
        assert_eq!(accounting.input_tokens, Some(10));
        assert_eq!(accounting.output_tokens, Some(5));
        assert_eq!(accounting.remote_calls, 1);
        accounting.add_request(&unavailable);
        assert_eq!(accounting.input_tokens, None);
        assert_eq!(accounting.output_tokens, None);
        assert_eq!(accounting.remote_calls, 2);
    }

    #[test]
    fn word_limit_extraction_is_number_near_noun() {
        assert_eq!(
            extract_per_item_word_limit("haceme un resumen de no más de 20 palabras por cada uno"),
            Some(20)
        );
        assert_eq!(
            extract_per_item_word_limit("resumí cada uno en 15 palabras"),
            Some(15)
        );
        assert_eq!(
            extract_per_item_word_limit("summarize each one in no more than 30 words"),
            Some(30)
        );
        assert_eq!(extract_per_item_word_limit("resumí cada uno"), None);
        assert_eq!(extract_per_item_word_limit("¿qué pasa con el 2026?"), None);
    }

    #[test]
    fn parse_extracts_each_key_and_drops_missing_ones() {
        let keys: Vec<String> = (1..=3).map(|i| format!("M{i:03}")).collect();
        let text =
            "M001: Resumen del archivo uno.\nM002: Resumen del archivo dos.\nM003: Otro resumen.";
        let parsed = parse_per_item_output(text, &keys, 10);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed["M001"], "Resumen del archivo uno.");
        assert_eq!(parsed["M003"], "Otro resumen.");

        let partial = "M001: primero\nM003: tercero";
        let parsed = parse_per_item_output(partial, &keys, 10);
        assert_eq!(parsed.len(), 2);
        assert!(!parsed.contains_key("M002"));
    }

    #[test]
    fn parse_enforces_the_word_limit_deterministically() {
        let keys = vec!["M001".to_owned()];
        let text = "M001: uno dos tres cuatro cinco seis siete ocho nueve diez once doce";
        let parsed = parse_per_item_output(text, &keys, 5);
        assert_eq!(parsed["M001"], "uno dos tres cuatro cinco");
    }

    #[test]
    fn parse_ignores_unknown_keys_and_prose_without_keys() {
        let keys: Vec<String> = (1..=2).map(|i| format!("M{i:03}")).collect();
        // Unknown key (M999) must not create a slot; prose without keys must not
        // create a slot.
        let parsed = parse_per_item_output("M999: inventado\nsin claves aquí", &keys, 40);
        assert!(parsed.is_empty());
        // Unknown key between real keys is ignored as a slot.
        let parsed = parse_per_item_output("M001: uno\nM999: inventado\nM002: dos", &keys, 40);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["M001"], "uno");
        assert_eq!(parsed["M002"], "dos");
    }

    #[test]
    fn parse_duplicate_expected_key_is_first_wins() {
        let keys = vec!["M001".to_owned(), "M002".to_owned()];
        let text = "M001: primero\nM002: dos\nM001: duplicado";
        let parsed = parse_per_item_output(text, &keys, 40);
        assert_eq!(parsed["M001"], "primero", "first occurrence wins");
        assert_eq!(parsed["M002"], "dos");
        assert_eq!(parsed.len(), 2);
    }

    #[test]
    fn parse_reordered_keys_map_to_the_right_key() {
        let keys: Vec<String> = (1..=3).map(|i| format!("M{i:03}")).collect();
        let text = "M003: tres\nM001: uno\nM002: dos";
        let parsed = parse_per_item_output(text, &keys, 40);
        assert_eq!(parsed["M001"], "uno");
        assert_eq!(parsed["M002"], "dos");
        assert_eq!(parsed["M003"], "tres");
    }

    #[test]
    fn parse_embedded_m_token_does_not_misattribute_content() {
        let keys = vec!["M001".to_owned(), "M002".to_owned()];
        // "M002" appears inside M001's prose, but it is not line-anchored, so it
        // must stay part of M001's summary and never truncate/misattribute.
        let text = "M001: este resumen menciona M002 de pasada.\nM002: resumen real";
        let parsed = parse_per_item_output(text, &keys, 40);
        assert_eq!(
            parsed["M001"], "este resumen menciona M002 de pasada.",
            "embedded token must stay inside its own summary"
        );
        assert_eq!(parsed["M002"], "resumen real");
    }

    #[test]
    fn parse_bare_m_token_without_delimiter_is_prose() {
        let keys = vec!["M001".to_owned()];
        // A line "M001 aparece en el texto" has no delimiter after the key, so it
        // is prose and must not open a slot.
        let parsed = parse_per_item_output("M001 aparece en el texto", &keys, 40);
        assert!(parsed.is_empty());
        // A numbered list line "1. ..." is also prose, not a key.
        let parsed = parse_per_item_output("1. primer punto", &keys, 40);
        assert!(parsed.is_empty());
    }

    #[test]
    fn render_uses_exact_source_names_one_per_entry() {
        let outcomes = vec![
            PerItemOutcome {
                material_id: "a".into(),
                source_name: "archivo-uno.md".into(),
                summary: "Resumen.".into(),
                fallback: false,
            },
            PerItemOutcome {
                material_id: "b".into(),
                source_name: "archivo-dos.md".into(),
                summary: PER_ITEM_SYNTHESIS_FAILED.into(),
                fallback: true,
            },
        ];
        let rendered = render_per_item_outcomes(&outcomes);
        assert_eq!(
            rendered,
            "1. archivo-uno.md: Resumen.\n2. archivo-dos.md: No se pudo generar el resumen de este archivo en esta ejecución."
        );
        assert!(!rendered.contains("Fuentes:"));
    }

    #[test]
    fn chunk_representative_is_deterministic_and_bounded() {
        let chunks: Vec<(String, String)> = (0..10)
            .map(|i| {
                (
                    format!("c{i}"),
                    format!("contenido del chunk {i} ").repeat(20),
                )
            })
            .collect();
        let representative = chunk_representative(&chunks);
        assert!(
            representative.chars().count()
                <= REPRESENTATIVE_MAX_CHUNKS * REPRESENTATIVE_CHUNK_CHARS + 20,
            "representative must stay bounded"
        );
        assert!(representative.contains("contenido del chunk 0"));
        assert!(representative.contains("contenido del chunk 5"));
        assert!(representative.contains("contenido del chunk 9"));
        let empty = chunk_representative(&[]);
        assert_eq!(empty, "");
    }

    #[test]
    fn request_keys_are_stable_and_instruction_is_explicit() {
        let items: Vec<PerItemMaterial> = (0..3)
            .map(|i| PerItemMaterial {
                key: format!("M{:03}", i + 1),
                material_id: format!("m{i}"),
                source_name: format!("archivo-{i}.md"),
                representative: "representante".into(),
                order: i,
            })
            .collect();
        let request = build_per_item_request(&items, 20);
        assert_eq!(request.labels.len(), 3);
        assert_eq!(request.labels[0].label, "M001");
        assert_eq!(request.labels[2].source_name, "archivo-2.md");
        assert!(request.instruction.contains("M001, M002"));
        assert!(request.instruction.contains("no más de 20 palabras"));
    }

    fn make_items(count: usize, chars: usize) -> Vec<PerItemMaterial> {
        (0..count)
            .map(|i| PerItemMaterial {
                key: format!("M{:03}", i + 1),
                material_id: format!("m{i}"),
                source_name: format!("archivo-{i}.md"),
                representative: "x".repeat(chars),
                order: i,
            })
            .collect()
    }

    #[test]
    fn oversized_representative_is_bounded_before_request_utf8_safe() {
        let huge = "á".repeat(5_000);
        let bounded = bound_representative(&huge);
        assert!(bounded.chars().count() <= PER_ITEM_REPRESENTATIVE_MAX_CHARS);
        // Truncation is a character-boundary prefix: valid UTF-8 and deterministic.
        let expected = huge
            .chars()
            .take(PER_ITEM_REPRESENTATIVE_MAX_CHARS)
            .collect::<String>();
        assert_eq!(bounded, expected);
        // A single bounded item always fits within the hard budget.
        let item = PerItemMaterial {
            key: "M001".into(),
            material_id: "m0".into(),
            source_name: "archivo.md".into(),
            representative: bounded,
            order: 0,
        };
        let request = build_per_item_request(&[item], 40);
        assert!(
            request.estimated_units()
                <= PerItemExecutionOptions::default().max_serialized_bytes_per_call
        );
    }

    #[test]
    fn packer_bounds_batches_by_items_and_hard_budget() {
        let options = PerItemExecutionOptions::default();
        // 1 small item -> 1 call.
        assert_eq!(pack_per_item_batches(&make_items(1, 60), options).len(), 1);
        // 50 normal items -> 1 call (under budget).
        assert_eq!(
            pack_per_item_batches(&make_items(50, 600), options).len(),
            1
        );
        // 100 normal items -> 1 call (under budget, at the item cap).
        assert_eq!(
            pack_per_item_batches(&make_items(100, 600), options).len(),
            1
        );
        // 101 items -> 2 batches (item cap).
        assert_eq!(
            pack_per_item_batches(&make_items(101, 600), options).len(),
            2
        );
        // 250 items -> 3 batches (item cap).
        assert_eq!(
            pack_per_item_batches(&make_items(250, 600), options).len(),
            3
        );
        // 100 large bounded (800-char) reused summaries -> split by budget.
        let large = pack_per_item_batches(&make_items(100, 800), options);
        assert!(large.len() > 1, "large items must split by the hard budget");
        // Every generated request stays within the hard budget (size, not call
        // count, is the invariant).
        for batch in large {
            let request = build_per_item_request(&batch, 40);
            assert!(
                request.estimated_units() <= options.max_serialized_bytes_per_call,
                "batch of {} items estimated {} bytes over budget {}",
                batch.len(),
                request.estimated_units(),
                options.max_serialized_bytes_per_call
            );
        }
    }

    #[test]
    fn packer_respects_a_small_injected_budget_deterministically() {
        let options = PerItemExecutionOptions {
            max_items_per_call: 100,
            max_words_per_item: 40,
            max_serialized_bytes_per_call: 1_500,
        };
        // 600-char representatives cost ~635 serialized bytes each plus the
        // ~405-byte instruction/header overhead, so exactly one item fits per
        // call under a 1500-byte budget.
        let batches = pack_per_item_batches(&make_items(10, 600), options);
        assert!(batches.len() > 1);
        for batch in &batches {
            let request = build_per_item_request(batch, 40);
            assert!(
                request.estimated_units() <= options.max_serialized_bytes_per_call,
                "injected budget must be enforced: {} > {}",
                request.estimated_units(),
                options.max_serialized_bytes_per_call
            );
        }
        // Preserve selected order across batches.
        let flattened: Vec<String> = batches.iter().flatten().map(|i| i.key.clone()).collect();
        let expected: Vec<String> = (0..10).map(|i| format!("M{:03}", i + 1)).collect();
        assert_eq!(flattened, expected);
    }

    #[test]
    fn oversized_single_item_is_shortened_instead_of_emitted_over_budget() {
        // A budget smaller than one full item must never produce an oversized
        // request or loop forever: the representative is deterministically
        // shortened so the single item still fits.
        let options = PerItemExecutionOptions {
            max_items_per_call: 100,
            max_words_per_item: 40,
            max_serialized_bytes_per_call: 800,
        };
        let items = make_items(1, 600);
        let batches = pack_per_item_batches(&items, options);
        assert_eq!(batches.len(), 1);
        let request = build_per_item_request(&batches[0], 40);
        assert!(
            request.estimated_units() <= options.max_serialized_bytes_per_call,
            "single item of {} bytes must be shortened to fit {}",
            request.estimated_units(),
            options.max_serialized_bytes_per_call
        );
        // The item was kept (never dropped), just with a shorter representative.
        assert_eq!(batches[0].len(), 1);
        assert!(batches[0][0].representative.chars().count() < 600);
    }
}
