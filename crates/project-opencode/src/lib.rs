//! Shared OpenCode serve process, loopback HTTP client, and isolated XDG env.

#![forbid(unsafe_code)]

mod backend;
mod error;
pub mod messages;
mod status;

use std::path::Path;

use serde_json::{Value, json};

pub use backend::OpenCodeBackend;
pub use error::{BackendError, BackendResult};
pub use status::BackendStatus;

/// Loopback-only argv for `opencode serve`. No shell involved: every element
/// is a literal token passed via `Command::args`.
pub fn build_argv(port: u16) -> Vec<String> {
    vec![
        "serve".into(),
        "--hostname".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "--pure".into(),
    ]
}

/// Append `directory` as a query parameter, matching OpenCode 1.18.25
/// (`POST /session?directory=`). A JSON body field named `directory` is not
/// in the create-session schema and is ignored, so the sidecar would otherwise
/// bind the session to its process cwd (the AppImage mount).
pub fn with_directory_query(path: &str, directory: &str) -> String {
    let encoded = encode_query_component(directory);
    if path.ends_with('?') {
        format!("{path}directory={encoded}")
    } else if path.contains('?') {
        format!("{path}&directory={encoded}")
    } else {
        format!("{path}?directory={encoded}")
    }
}

/// The shared product-session permission ruleset for **ordinary chat** sessions
/// (ADR-0006): deny `external_directory` so the agent is confined to its
/// session directory and cannot reach other projects, the home directory, or
/// secrets.
///
/// Ordinary chat keeps the full coding tool set (the `build` agent's own
/// permission defaults plus `--auto`), so this ruleset must NOT be used for
/// scratch classification/summarization sessions. Those need the tool-safe
/// contract; use [`scratch_tool_free_permission`] instead.
pub fn external_directory_deny_permission() -> Value {
    json!([
        {
            "permission": "external_directory",
            "pattern": "*",
            "action": "deny",
        }
    ])
}

/// Builtin OpenCode 1.18.25 coding/network/subagent tools (plus `question`)
/// that scratch classifier/summarizer sessions must not *execute*. Names come
/// from `packages/opencode/src/tool/registry.ts` and the permission docs for
/// that sidecar; `edit` also covers `write` / `apply_patch` at ask-time.
const SCRATCH_DENIED_TOOLS: &[&str] = &[
    "bash",
    "read",
    "edit",
    "glob",
    "grep",
    "task",
    "skill",
    "lsp",
    "webfetch",
    "websearch",
    "todowrite",
    "question",
];

/// Pattern that still wildcard-matches every real tool invocation in OpenCode
/// 1.18.25, but is **not** the exact string `"*"`.
///
/// `Permission.disabled` hides a tool from the model only when the last
/// matching rule has `pattern === "*"` (exact). A global `permission:"*"`
/// `action:"deny"` ruleset, or per-tool `"*"` denies that empty `activeTools`,
/// produces an assistant completed with no text on this sidecar (live A/B:
/// ordinary `external_directory` deny → text; `"*"` deny → empty;
/// per-tool `"*"` deny → empty; per-tool `"**"` deny → text).
const SCRATCH_TOOL_EXECUTION_DENY_PATTERN: &str = "**";

/// The **tool-safe** permission ruleset for scratch sessions (classifier and
/// summarizer/K6). It is the `permission` body of `POST /session`.
///
/// OpenCode 1.18.25 builds the effective ruleset as
/// `Permission.merge(agent.permission, session.permission)` (a concatenation)
/// and evaluates with **last-match-wins** (`Permission.evaluate` uses
/// `findLast`). The `build` agent's permission *starts* with
/// `{"permission":"*","pattern":"*","action":"allow"}`, so a scratch session
/// that only denied `external_directory` would leave every coding tool
/// auto-approved at execution time.
///
/// This ruleset therefore appends an explicit `deny` for each builtin coding
/// tool, using pattern `"**"` so execution is blocked without removing the
/// tools from the model inventory. The `invalid` repair tool is intentionally
/// not denied. `external_directory` stays `"*"` deny so the session cannot
/// leave its directory. There is **no** global `permission:"*"` deny: that
/// ruleset is the proven cause of empty scratch assistants on OpenCode 1.18.25.
pub fn scratch_tool_free_permission() -> Value {
    let mut rules: Vec<Value> = SCRATCH_DENIED_TOOLS
        .iter()
        .map(|permission| {
            json!({
                "permission": permission,
                "pattern": SCRATCH_TOOL_EXECUTION_DENY_PATTERN,
                "action": "deny",
            })
        })
        .collect();
    rules.push(json!({
        "permission": "external_directory",
        "pattern": "*",
        "action": "deny",
    }));
    Value::Array(rules)
}

fn encode_query_component(value: &str) -> String {
    let mut encoded = String::new();
    for &byte in value.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

/// Child environment with the user's environment cleared (by the supervisor)
/// and replaced by PATH/HOME plus isolated XDG dirs under `config_dir`.
pub fn build_env(config_dir: &Path) -> Vec<(String, String)> {
    let mut env = vec![
        ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
        ("HOME".into(), std::env::var("HOME").unwrap_or_default()),
        ("XDG_CONFIG_HOME".into(), config_dir.display().to_string()),
        (
            "XDG_DATA_HOME".into(),
            config_dir.join("data").display().to_string(),
        ),
        (
            "XDG_CACHE_HOME".into(),
            config_dir.join("cache").display().to_string(),
        ),
        (
            "XDG_STATE_HOME".into(),
            config_dir.join("state").display().to_string(),
        ),
    ];
    env.extend(windows_systemroot_env());
    env
}

/// On Windows the reconstructed child environment must carry the parent
/// `SYSTEMROOT` value: OpenCode's startup aborts immediately (0xC0000409) when
/// it is missing after `env_clear`. Only that single variable is forwarded; no
/// other parent variable is inherited. Non-Windows targets forward nothing.
#[cfg(windows)]
fn windows_systemroot_env() -> Vec<(String, String)> {
    vec![(
        "SYSTEMROOT".into(),
        std::env::var("SYSTEMROOT").unwrap_or_default(),
    )]
}

#[cfg(not(windows))]
fn windows_systemroot_env() -> Vec<(String, String)> {
    Vec::new()
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Semver(u64, u64, u64);

impl Semver {
    pub fn parse(raw: &str) -> Option<Self> {
        let trimmed = raw.trim().trim_start_matches(['v', 'V', '>', '=', '<']);
        let core = trimmed.split(['-', '+', ' ']).next().unwrap_or("");
        if core.is_empty() {
            return None;
        }
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let patch = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some(Semver(major, minor, patch))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_env, external_directory_deny_permission, scratch_tool_free_permission,
        with_directory_query,
    };
    use std::collections::BTreeSet;
    use std::path::Path;

    #[test]
    fn ordinary_chat_permission_denies_only_external_directory_never_star() {
        let rules = external_directory_deny_permission();
        let rules = rules.as_array().expect("permission must be an array");
        assert_eq!(rules.len(), 1, "must contain exactly one rule");
        assert_eq!(rules[0]["permission"], "external_directory");
        assert_eq!(rules[0]["pattern"], "*");
        assert_eq!(rules[0]["action"], "deny");
        assert!(
            rules.iter().all(|rule| rule["permission"] != "*"),
            "ordinary chat keeps the coding tool set; it must never emit a global * deny"
        );
    }

    #[test]
    fn scratch_permission_is_tool_safe_with_external_directory_deny() {
        let rules = scratch_tool_free_permission();
        let rules = rules.as_array().expect("permission must be an array");
        assert!(
            rules.iter().all(|rule| rule["permission"] != "*"),
            "scratch must not emit a global * permission rule"
        );
        let last = rules.last().expect("ruleset must not be empty");
        assert_eq!(last["permission"], "external_directory");
        assert_eq!(last["pattern"], "*");
        assert_eq!(last["action"], "deny");
        for tool in [
            "bash",
            "read",
            "edit",
            "glob",
            "grep",
            "task",
            "skill",
            "lsp",
            "webfetch",
            "websearch",
            "todowrite",
            "question",
        ] {
            assert!(
                rules.iter().any(|rule| {
                    rule["permission"] == tool
                        && rule["pattern"] == "**"
                        && rule["action"] == "deny"
                }),
                "{tool} must be execution-denied with pattern **"
            );
        }
    }

    #[test]
    fn directory_query_percent_encodes_path_separators() {
        let path = with_directory_query("/session", "/tmp/proj-7/workspace");
        assert_eq!(path, "/session?directory=%2Ftmp%2Fproj-7%2Fworkspace");
    }

    #[test]
    fn directory_query_appends_to_existing_query() {
        let path = with_directory_query("/session?limit=1", "/home/a");
        assert_eq!(path, "/session?limit=1&directory=%2Fhome%2Fa");
    }

    #[test]
    fn directory_query_does_not_insert_ampersand_after_trailing_question_mark() {
        let path = with_directory_query("/session?", "/tmp/a");
        assert_eq!(path, "/session?directory=%2Ftmp%2Fa");
    }

    #[test]
    fn directory_query_percent_encodes_ampersand_equals_plus_space_and_non_ascii() {
        let path = with_directory_query("/session", "/tmp/a&b=c+d e/café");
        assert_eq!(
            path,
            "/session?directory=%2Ftmp%2Fa%26b%3Dc%2Bd%20e%2Fcaf%C3%A9"
        );
    }

    #[test]
    fn build_env_preserves_systemroot_only_on_windows() {
        let config_dir = Path::new("/tmp/educai-env-contract/opencode-config");
        let env = build_env(config_dir);
        let keys: BTreeSet<&str> = env.iter().map(|(k, _)| k.as_str()).collect();

        #[allow(unused_mut)] // Windows adds SYSTEMROOT below.
        let mut expected: BTreeSet<&str> = [
            "PATH",
            "HOME",
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ]
        .into_iter()
        .collect();

        #[cfg(windows)]
        {
            expected.insert("SYSTEMROOT");
            let parent = std::env::var("SYSTEMROOT").unwrap_or_default();
            let value = env
                .iter()
                .find(|(k, _)| k == "SYSTEMROOT")
                .map(|(_, v)| v.as_str());
            assert_eq!(
                value,
                Some(parent.as_str()),
                "SYSTEMROOT must be forwarded verbatim from the parent env"
            );
        }

        #[cfg(not(windows))]
        {
            assert!(
                !keys.contains("SYSTEMROOT"),
                "SYSTEMROOT must not be forwarded on non-Windows targets"
            );
        }

        assert_eq!(
            keys, expected,
            "child env must contain exactly the managed keys, never arbitrary parent variables"
        );
    }

    /// A faithful, minimal model of OpenCode 1.18.25's permission merge and
    /// evaluation semantics, used only to prove that the scratch permission
    /// ruleset is genuinely tool-free after it is merged with the `build`
    /// agent's own rules.
    ///
    /// The three contracts mirrored here (all confirmed against the OpenCode
    /// `packages/opencode/src/permission` source):
    ///
    /// - `merge(...)` concatenates rulesets (`rulesets.flat()`).
    /// - `evaluate(permission, pattern, ruleset)` returns the action of the
    ///   LAST rule whose `permission` and `pattern` both wildcard-match the
    ///   input, defaulting to `ask`.
    /// - `disabled(tools, ruleset)` removes a tool when the LAST rule matching
    ///   its permission key is a `*`-pattern `deny` (write/edit/patch/multiedit
    ///   collapse to the `edit` permission key).
    mod permission_model {
        use crate::scratch_tool_free_permission;

        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        enum Action {
            Allow,
            Deny,
            Ask,
        }

        #[derive(Clone, Debug)]
        struct Rule {
            permission: String,
            pattern: String,
            action: Action,
        }

        /// Mirrors OpenCode 1.18.25 `Permission.disabled`: write/apply_patch
        /// collapse to the `edit` permission key.
        const EDIT_TOOLS: &[&str] = &["edit", "write", "apply_patch"];

        fn action_of(raw: &str) -> Action {
            match raw {
                "allow" => Action::Allow,
                "deny" => Action::Deny,
                _ => Action::Ask,
            }
        }

        fn rule(permission: &str, pattern: &str, action: Action) -> Rule {
            Rule {
                permission: permission.to_owned(),
                pattern: pattern.to_owned(),
                action,
            }
        }

        /// Mirrors `Wildcard.match`: backslashes normalized to `/`, `*` matches
        /// any sequence (including empty), `?` matches one character, anchored.
        fn wildcard_match(value: &str, pattern: &str) -> bool {
            let value = value.replace('\\', "/");
            let pattern = pattern.replace('\\', "/");
            glob_match(value.as_bytes(), pattern.as_bytes())
        }

        fn glob_match(value: &[u8], pattern: &[u8]) -> bool {
            let (mut v, mut p) = (0usize, 0usize);
            let (mut star, mut mark) = (None, 0usize);
            while v < value.len() {
                if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == value[v]) {
                    v += 1;
                    p += 1;
                } else if p < pattern.len() && pattern[p] == b'*' {
                    star = Some(p);
                    mark = v;
                    p += 1;
                } else if let Some(star_idx) = star {
                    p = star_idx + 1;
                    mark += 1;
                    v = mark;
                } else {
                    return false;
                }
            }
            while p < pattern.len() && pattern[p] == b'*' {
                p += 1;
            }
            p == pattern.len()
        }

        /// `Permission.merge(...rulesets)` = flat concatenation, in order.
        fn merge(rulesets: &[Vec<Rule>]) -> Vec<Rule> {
            rulesets.iter().flatten().cloned().collect()
        }

        /// `Permission.evaluate(permission, pattern, ruleset)` = last match wins.
        fn evaluate(permission: &str, pattern: &str, ruleset: &[Rule]) -> Action {
            ruleset
                .iter()
                .rev()
                .find(|rule| {
                    wildcard_match(permission, &rule.permission)
                        && wildcard_match(pattern, &rule.pattern)
                })
                .map(|rule| rule.action)
                .unwrap_or(Action::Ask)
        }

        /// `Permission.disabled(tools, ruleset)` = tools whose last matching
        /// rule has **exact** `pattern == "*"` and `action == deny`.
        fn disabled(tools: &[&str], ruleset: &[Rule]) -> Vec<String> {
            tools
                .iter()
                .filter(|tool| {
                    let permission = if EDIT_TOOLS.contains(tool) {
                        "edit"
                    } else {
                        *tool
                    };
                    ruleset
                        .iter()
                        .rev()
                        .find(|rule| wildcard_match(permission, &rule.permission))
                        .is_some_and(|rule| rule.pattern == "*" && rule.action == Action::Deny)
                })
                .map(|tool| (*tool).to_owned())
                .collect()
        }

        /// The `build` agent's permission ruleset as OpenCode 1.18.25 derives it:
        /// `merge(defaults, {question:"allow", plan_enter:"allow"}, user)`. The
        /// isolated product config carries no user rules. The important facts
        /// are the leading `*: allow` and the `external_directory: ask` default.
        fn build_agent_permission() -> Vec<Rule> {
            vec![
                rule("*", "*", Action::Allow),
                rule("doom_loop", "*", Action::Ask),
                rule("external_directory", "*", Action::Ask),
                rule("question", "*", Action::Deny),
                rule("plan_enter", "*", Action::Deny),
                rule("plan_exit", "*", Action::Deny),
                rule("read", "*", Action::Allow),
                rule("read", "*.env", Action::Ask),
                rule("read", "*.env.*", Action::Ask),
                rule("read", "*.env.example", Action::Allow),
                rule("question", "*", Action::Allow),
                rule("plan_enter", "*", Action::Allow),
            ]
        }

        /// Parses the PRODUCTION [`scratch_tool_free_permission`] JSON into
        /// model rules so the security tests exercise the exact bytes the
        /// classifier/summarizer will send, not a duplicated fixture.
        fn scratch_rules() -> Vec<Rule> {
            let value = scratch_tool_free_permission();
            value
                .as_array()
                .expect("scratch permission is an array")
                .iter()
                .map(|entry| {
                    let permission = entry["permission"].as_str().expect("permission");
                    let pattern = entry["pattern"].as_str().expect("pattern");
                    let action = entry["action"].as_str().expect("action");
                    rule(permission, pattern, action_of(action))
                })
                .collect()
        }

        /// The exact effective ruleset OpenCode evaluates for a scratch session
        /// running under the `build` agent.
        fn effective_scratch_ruleset() -> Vec<Rule> {
            merge(&[build_agent_permission(), scratch_rules()])
        }

        const EXECUTION_DENIED: &[&str] = &[
            "bash",
            "read",
            "edit",
            "glob",
            "grep",
            "task",
            "skill",
            "lsp",
            "webfetch",
            "websearch",
            "todowrite",
            "question",
            "external_directory",
        ];

        const REPRESENTATIVE_TOOLS: &[&str] = &[
            "bash",
            "read",
            "edit",
            "write",
            "apply_patch",
            "webfetch",
            "invalid",
        ];

        #[test]
        fn build_agent_alone_auto_allows_tools() {
            let ruleset = build_agent_permission();
            // The security premise: without the scratch deny, the build agent's
            // leading `*: allow` leaves these tools allowed.
            assert_eq!(evaluate("bash", "*", &ruleset), Action::Allow);
            assert_eq!(evaluate("read", "*", &ruleset), Action::Allow);
            assert_eq!(evaluate("edit", "*", &ruleset), Action::Allow);
            assert_eq!(evaluate("webfetch", "*", &ruleset), Action::Allow);
        }

        #[test]
        fn scratch_denies_bash() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(evaluate("bash", "*", &ruleset), Action::Deny);
        }

        #[test]
        fn scratch_denies_read() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(evaluate("read", "*", &ruleset), Action::Deny);
        }

        #[test]
        fn scratch_denies_edit() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(evaluate("edit", "*", &ruleset), Action::Deny);
        }

        #[test]
        fn scratch_denies_webfetch() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(evaluate("webfetch", "*", &ruleset), Action::Deny);
        }

        #[test]
        fn scratch_does_not_use_global_star_deny() {
            let scratch = scratch_rules();
            assert!(scratch.iter().all(|rule| rule.permission != "*"));
        }

        #[test]
        fn scratch_denies_external_directory() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(
                evaluate("external_directory", "/etc/passwd", &ruleset),
                Action::Deny
            );
        }

        #[test]
        fn scratch_execution_denies_coding_tools_without_hiding_them() {
            let ruleset = effective_scratch_ruleset();
            for permission in EXECUTION_DENIED {
                assert_eq!(
                    evaluate(permission, "*", &ruleset),
                    Action::Deny,
                    "{permission} must be denied at execution time"
                );
            }
            let hidden = disabled(REPRESENTATIVE_TOOLS, &ruleset);
            assert!(
                hidden.is_empty(),
                "pattern ** must not hide tools via Permission.disabled, got {hidden:?}"
            );
        }

        #[test]
        fn scratch_keeps_invalid_repair_tool_visible() {
            let ruleset = effective_scratch_ruleset();
            assert_eq!(evaluate("invalid", "*", &ruleset), Action::Allow);
            assert!(!disabled(&["invalid"], &ruleset).contains(&"invalid".to_owned()));
        }

        #[test]
        fn build_agent_cannot_override_scratch_deny() {
            // OpenCode merges agent rules first and session rules last, and
            // evaluates last-match-wins, so the session-level deny is final.
            let ruleset = effective_scratch_ruleset();
            for permission in ["bash", "read", "edit", "webfetch"] {
                assert_eq!(evaluate(permission, "*", &ruleset), Action::Deny);
            }
        }

        #[test]
        fn merge_order_is_what_keeps_the_policy_final() {
            let agent = build_agent_permission();
            let scratch = scratch_rules();
            // Correct order (agent then session): scratch deny wins.
            assert_eq!(
                evaluate("bash", "*", &merge(&[agent.clone(), scratch.clone()])),
                Action::Deny
            );
            // Reversed order would let the agent's `*: allow` win again. This is
            // why the scratch deny MUST be the session (last) ruleset, never the
            // agent's, and why it is appended after the agent's own rules.
            assert_eq!(
                evaluate("bash", "*", &merge(&[scratch, agent])),
                Action::Allow
            );
        }
    }
}
