//! Bounded EducAI conversation reconstruction for ephemeral Knowledge sessions.
//!
//! OpenCode session identity is not the continuity store for Knowledge answers.
//! Visible user/assistant text already persisted by EducAI may be restated into
//! an ephemeral Knowledge prompt so a follow-up like "eso" remains resolvable
//! without inheriting raw RAG evidence from a reused OpenCode transcript.
//!
//! Limits are hard: a handful of recent visible turns, truncated, never the
//! current user prompt, never Knowledge evidence markup, never the whole corpus.

const MAX_CONTEXT_MESSAGES: usize = 4;
const MAX_CHARS_PER_MESSAGE: usize = 480;
const MAX_CONTEXT_CHARS: usize = 1_400;

/// One already-persisted visible chat message. Structural ids and flags only
/// travel with the text the user already saw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisibleChatMessage<'a> {
    pub id: &'a str,
    pub is_user: bool,
    pub ok: bool,
    pub text: &'a str,
}

/// Compact visible history plus structural counts (never extra content).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConversationContextReport {
    pub text: String,
    pub message_count: usize,
    pub char_count: usize,
}

/// Compact visible history for an ephemeral Knowledge prompt, or `None` when
/// there is nothing bounded and useful to restate.
pub fn bounded_conversation_context(
    messages: &[VisibleChatMessage<'_>],
    current_user_message_id: Option<&str>,
) -> Option<String> {
    bounded_conversation_context_report(messages, current_user_message_id).map(|report| report.text)
}

/// Same reconstruction as [`bounded_conversation_context`], with counts for
/// structural telemetry.
pub fn bounded_conversation_context_report(
    messages: &[VisibleChatMessage<'_>],
    current_user_message_id: Option<&str>,
) -> Option<ConversationContextReport> {
    let mut selected = Vec::new();
    for message in messages.iter().rev() {
        if current_user_message_id.is_some_and(|id| message.id == id) {
            continue;
        }
        if !message.ok {
            continue;
        }
        let text = visible_excerpt(message.text);
        if text.is_empty() {
            continue;
        }
        selected.push((message.is_user, text));
        if selected.len() == MAX_CONTEXT_MESSAGES {
            break;
        }
    }
    if selected.is_empty() {
        return None;
    }
    selected.reverse();

    let mut out = String::from(
        "Conversación reciente visible (referencia acotada; no es evidencia documental):\n",
    );
    let mut message_count = 0usize;
    for (is_user, text) in selected {
        let role = if is_user { "Usuario" } else { "Asistente" };
        let line = format!("{role}: {text}\n");
        if out.chars().count() + line.chars().count() > MAX_CONTEXT_CHARS {
            break;
        }
        out.push_str(&line);
        message_count += 1;
    }
    let trimmed = out.trim();
    if trimmed.ends_with(':') || message_count == 0 {
        return None;
    }
    Some(ConversationContextReport {
        char_count: trimmed.chars().count(),
        text: trimmed.to_owned(),
        message_count,
    })
}

fn visible_excerpt(text: &str) -> String {
    let without_evidence = strip_knowledge_evidence(text);
    let collapsed = without_evidence
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if collapsed.chars().count() <= MAX_CHARS_PER_MESSAGE {
        return collapsed;
    }
    let truncated: String = collapsed.chars().take(MAX_CHARS_PER_MESSAGE).collect();
    format!("{truncated}…")
}

fn strip_knowledge_evidence(text: &str) -> String {
    let mut remaining = text;
    let mut out = String::with_capacity(text.len());
    while let Some(start) = remaining.find("<knowledge_evidence") {
        out.push_str(&remaining[..start]);
        remaining = &remaining[start..];
        match remaining.find("</knowledge_evidence>") {
            Some(end) => {
                remaining = &remaining[end + "</knowledge_evidence>".len()..];
            }
            None => {
                remaining = "";
                break;
            }
        }
    }
    out.push_str(remaining);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg<'a>(id: &'a str, is_user: bool, text: &'a str) -> VisibleChatMessage<'a> {
        VisibleChatMessage {
            id,
            is_user,
            ok: true,
            text,
        }
    }

    #[test]
    fn omits_the_current_user_turn_and_keeps_the_prior_pair() {
        let messages = [
            msg("u1", true, "¿Qué dijeron las reuniones sobre Kubernetes?"),
            msg("a1", false, "Hablaron de orquestación de contenedores."),
            msg("u2", true, "¿Y cómo se relaciona eso con OpenShift?"),
        ];
        let context = bounded_conversation_context(&messages, Some("u2")).unwrap();
        assert!(context.contains("Kubernetes"));
        assert!(context.contains("orquestación"));
        assert!(!context.contains("OpenShift"));
        assert!(!context.contains("<knowledge_evidence"));
    }

    #[test]
    fn strips_raw_knowledge_evidence_if_it_ever_appeared() {
        let poisoned = concat!(
            "Resumen corto.\n",
            "<knowledge_evidence trust=\"untrusted\">SECRET CHUNK BODY</knowledge_evidence>"
        );
        let messages = [msg("u1", true, "pregunta"), msg("a1", false, poisoned)];
        let context = bounded_conversation_context(&messages, None).unwrap();
        assert!(context.contains("Resumen corto"));
        assert!(!context.contains("SECRET CHUNK BODY"));
        assert!(!context.contains("<knowledge_evidence"));
    }

    #[test]
    fn ignores_failed_messages_and_empty_history() {
        let failed = VisibleChatMessage {
            id: "a1",
            is_user: false,
            ok: false,
            text: "falló",
        };
        assert!(bounded_conversation_context(&[failed], None).is_none());
        assert!(bounded_conversation_context(&[], Some("u1")).is_none());
    }

    #[test]
    fn truncates_oversized_visible_turns() {
        let huge = "Kubernetes ".repeat(200);
        let messages = [msg("u1", true, &huge)];
        let context = bounded_conversation_context(&messages, None).unwrap();
        assert!(context.chars().count() < 1_800);
        assert!(context.contains('…'));
        assert!(!context.contains(&huge));
    }

    #[test]
    fn keeps_at_most_four_visible_messages_in_order() {
        let messages = [
            msg("u0", true, "cero"),
            msg("a0", false, "respuesta cero"),
            msg("u1", true, "uno"),
            msg("a1", false, "respuesta uno"),
            msg("u2", true, "dos"),
        ];
        let report = bounded_conversation_context_report(&messages, None).unwrap();
        assert_eq!(report.message_count, 4);
        assert!(!report.text.contains("Usuario: cero"));
        assert!(report.text.contains("respuesta cero"));
        assert!(report.text.contains("uno"));
        assert!(report.text.contains("dos"));
        let usuario = report.text.find("Usuario: uno").unwrap();
        let asistente = report.text.find("Asistente: respuesta uno").unwrap();
        let last = report.text.find("Usuario: dos").unwrap();
        assert!(usuario < asistente && asistente < last);
        assert!(report.char_count <= 1_400);
        assert_eq!(report.char_count, report.text.chars().count());
    }

    #[test]
    fn utf8_truncation_does_not_split_scalar_values() {
        let huge = "áéíóú😀".repeat(200);
        let messages = [msg("u1", true, &huge)];
        let context = bounded_conversation_context(&messages, None).unwrap();
        assert!(context.contains('…'));
        assert!(std::str::from_utf8(context.as_bytes()).is_ok());
        assert!(context.chars().all(|ch| ch != '\u{FFFD}'));
        let excerpt = context
            .lines()
            .find(|line| line.starts_with("Usuario:"))
            .unwrap();
        let body = excerpt
            .trim_start_matches("Usuario: ")
            .trim_end_matches('…');
        assert!(body.chars().count() <= 480);
    }

    #[test]
    fn omits_internal_prompts_chunks_and_scratch_bodies() {
        let poisoned = concat!(
            "Respuesta visible.\n",
            "<knowledge_evidence trust=\"untrusted\">CHUNK SECRET</knowledge_evidence>\n",
        );
        let messages = [
            msg("u1", true, "pregunta"),
            msg("a1", false, poisoned),
            msg("u2", true, "follow"),
        ];
        let context = bounded_conversation_context(&messages, Some("u2")).unwrap();
        assert!(context.contains("Respuesta visible"));
        assert!(!context.contains("CHUNK SECRET"));
        assert!(!context.contains("<knowledge_evidence"));
        assert!(!context.contains("follow"));
        assert!(!context.contains("[M001]"));
        assert!(!context.contains("You are a classifier"));
    }
}
