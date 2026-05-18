---
module: core::recommend
file: src/core/recommend.rs
role: feature
---

# core::recommend — LLM skill router

## Purpose
Opt-in skill auto-routing. A small LLM (default `deepseek-v4-flash` via OpenAI-compatible API) looks at the user prompt + the list of installed skills (name + AI summary + tags) and returns the top-K most relevant skills plus a one-sentence `reasoning:` line. The `UserPromptSubmit` hook injects this as candidate list + activation command into the main Claude Code prompt; the main agent then runs `runai recommend get <name>` to fetch the SKILL.md atomically. `recommend get` is the **single source of truth** for "skill adopted" — no PostToolUse hook, no transcript scanning, no self-report.

Disabled by default. User must run `runai recommend setup` (interactive) or write `~/.runai/config.toml` manually before any LLM call happens.

## Public API
- `struct RecommendConfig` — fields: `enabled`, `provider`, `base_url`, `model`, `api_key`, `top_k` (default 8 — soft ceiling on candidates surfaced per turn), `min_prompt_len`, `summary_lang`, `session_mode`, `session_history_limit`. Defaults: disabled, openai-compat, DeepSeek endpoint, `deepseek-v4-flash`, top_k=8, session_mode=Oneshot, history_limit=20.
- `enum Provider` — `OpenaiCompat` (default) / `Anthropic` / `ClaudeCli`.
- `enum SessionMode` — `Oneshot` (default, every call independent — prefix cache fully hits) / `Conversation` (replay this session's prior `(user, assistant)` turns from `router_events` so the LLM remembers what it already pushed; more tokens, more recall).
- `enum RouterMode` — `Compatible` (co-loadable workflow set, EXCLUSIVE picks 1-3 for user to choose).
- `RecommendConfig::load(paths)` / `save(paths)` — toml at `~/.runai/config.toml`. Save sets `0o600` on unix.
- `RecommendConfig::effective_api_key()` — config field first, then `RUNAI_RECOMMEND_API_KEY` env.
- `recommend(mgr, prompt, transcript_path, session_id, cwd) -> RouterDecision` — top-level entry. Builds BM25 prefilter → top-K candidates with AI summaries → LLM router call. When `cfg.session_mode == Conversation` and `session_id` is non-empty, prior `(user_msg, llm_raw_response)` pairs from `router_events` are threaded into the LLM messages array.
- `struct RouterTurn { user, assistant }` — one prior router round-trip. Conversation mode replays a `Vec<RouterTurn>` ahead of the current user msg.
- `struct RouterDecision { mode, reasoning, skills }` — `reasoning` is the LLM's one-sentence "why this set" (rendered into hook output as `router 判断：...`; hidden when empty).
- `struct RecommendedSkill { name, description }` — **no path, no content**. Activation is exclusively via `runai recommend get <name>`; the router never ships SKILL.md bytes or filesystem paths to the main agent.
- `format_for_hook(decision)` / `format_for_hook_with_session(decision, sid)` / `format_for_hook_full(decision, sid, history)` — three thin wrappers around `render_hook_output`, the **single** template-driven renderer for `hook_output.md`. Replaces the previous `format_compatible_set` + `format_pointer` + `format_multi` triple — one template, three `{ACTIVATION_DIRECTIVE}` strings, zero per-mode branches.
- `recent_transcript_messages(path, n)` — read the last `n` user/assistant text messages from a Claude Code transcript jsonl. Tool calls/results filtered out.

## Key invariants
- **One activation path, one counting path.** `runai recommend get <skill>` is the only command that bumps `usage_count` and writes a `router_session_adoptions` row. There is no PostToolUse hook, no transcript-scan fallback, no `runai recommend used` self-report — all three were removed. The router itself **never** bumps counts; recommending ≠ adopting.
- **Hook output never contains a filesystem path or `SKILL.md` body.** The `hook_output.md` template only references `runai recommend get <skill_name>` as a literal command for the agent to fill in. Tests assert `!out.contains("/skills/")` and `!out.contains("Source path")`.
- **Hook output is purely positive phrased.** No "不要 Read / sm_enable / sm_install" reverse directives — `LLM` follows positive single-path instructions much more reliably than negative restrictions. The template tells the agent only what to do, never what to avoid.
- **Disabled by default.** `RecommendConfig::default().enabled == false`. Loading a missing config returns default. `recommend()` returns an empty `RouterDecision` when disabled — no LLM call, no network, no log.
- **Per-session de-duplication is enforced on the wire, not only in the prompt.** When `session_id` is present, `db.router_session_recommended_skills(sid)` returns every skill name routed this session. Two defenses:
  1. Inject `ALREADY_ROUTED` block into the LLM user message — system prompt instructs the model to skip these unless current prompt finally matches an older recommendation.
  2. Post-process: hallucinated / already-routed names are dropped before `render_hook_output` runs.
- **Mode + reasoning come from the LLM, defaults are safe.** First line of LLM output = `COMPATIBLE` / `EXCLUSIVE`; second line (optional) = `reasoning: ...`; remaining lines = skill names. Missing tag → `Exclusive` (safer — main agent will ask user to pick). Missing reasoning → empty string, the renderer hides the block.
- **LLM names filtered against installed skills.** Names from the model are intersected with `list_resources(Skill, _)`; hallucinated names are dropped silently.
- **`top_k = 8` is a soft cap, not a target.** Router is told via `recommend_user.md` to pick "by workflow need" — COMPATIBLE workflows can use 4-6 互补 skills, EXCLUSIVE picks 1-3 for the user to choose. 8 is the hard ceiling.
- **API key never logged or echoed.** `recommend status` shows only `set in config` / `set via env` / `missing`. Config file is `0o600`.
- **Returns success even when LLM call fails.** Errors go to stderr prefixed with `# runai recommend skipped:`; hook stdout stays parseable; main Claude continues unimpaired. Failed call is persisted with `status='error'`.

## Touch points
- **Upstream**: `cli::Commands::Recommend` dispatch. Subcommands: `runai recommend <prompt>` / `setup` / `status` / `hook-snippet` / `install-hook` / `uninstall-hook` / `stats` / `feedback` / **`get <skill>`** (the activation command) / `enrich` / `reset-scoring`.
- **Downstream**: `SkillManager::list_resources`, `AppPaths::{config_path, skills_dir}`, `reqwest::blocking::Client` (POSTs to `{base_url}/chat/completions` for openai-compat or `{base_url}/v1/messages` for anthropic).
- **External integration**: Claude Code's `UserPromptSubmit` hook in `~/.claude/settings.json`. Single hook entry `runai recommend`; no PostToolUse pairing any more. `install_claude_hook` proactively removes any legacy `runai recommend post-tool` entry it finds.

## Prompts (templates)
All in `src/core/prompts/`:
- `recommend_system.md` — router system prompt: intent extraction, identify-and-skip-quoted-history rule, COMPATIBLE-first decision tree, `[llm:N] [used:N] [group:X]` tag semantics, output format (mode + reasoning + names), 7 few-shot examples, ALREADY_ROUTED handling, and Conversation-mode guidance for using prior turns.
- `recommend_user.md` — user message scaffold; `{USER_PROMPT}` repeated head + tail to keep the real intent on top of mind after the candidate list.
- `recommend_enrich.md` — per-skill enrichment prompt (6-line summary: task / triggers / inputs / outputs / not-for / score).
- `recommend_already_routed.md` — `{ALREADY_ROUTED}` block included in user msg.
- `recommend_cwd_prefix.md`, `recommend_history_prefix.md`, `recommend_project_context.md` — supporting blocks.
- **`hook_output.md`** — single unified template for everything the main Claude agent sees. `{MODE}`, `{REASONING_BLOCK}`, `{CANDIDATES_BLOCK}`, `{SESSION_ID}`, `{ACTIVATION_DIRECTIVE}`, `{SESSION_HISTORY_BLOCK}`, `{FEEDBACK_PROTOCOL_BLOCK}` — replaces the previous `hook_pointer.md` + `hook_multi.md` pair.

## AI summary enrichment
- `enrich_skills(mgr, limit, mode, verbose, concurrency, only_names)` — generates bilingual AI summaries via the configured LLM. Each summary is stored in `resource_ai_summary` (DB) alongside an `llm_score` (0-10) used by the hybrid BM25 prefilter. Summary feeds BM25 retriever + routing LLM, not user-facing text.
- `EnrichMode::{MissingOnly, Stale, Force}` controls which skills are picked up. `only_names: Option<&[String]>` overrides `mode` to `Force` for the listed names — `runai install` / `scan` / `market-install` use this to refresh just the freshly-changed skills.
- `parse_enrich_response` requires a `task:` line — defensive against LLMs going off-format on huge SKILL.md.
- `reevaluate_skill(mgr, skill_name, feedback_note)` — single-skill re-enrich with explicit user feedback folded in. Adjusts `llm_score` + `summary` so routing signal evolves. Triggered by `runai recommend feedback <skill> --note "..."` and the hook output's feedback protocol footer (main Claude calls it on explicit user reaction only).

## Gotchas
- Anthropic provider hits `{base_url}/v1/messages` — pass host without `/v1`. Openai-compat hits `{base_url}/chat/completions` — base_url already includes the version segment.
- `Provider::ClaudeCli` stays oneshot even in Conversation mode (each call boots a full Claude Code session, so history replay would defeat the cost story).
- Hook stdout becomes part of the main Claude's prompt. The unified `hook_output.md` template + per-skill description (AI summary, not raw SKILL.md) keeps it well under Claude Code's 10 KB hook cap — even at top_k=8 the output is ~2-3 KB.
- Setup prompts for API key in plain text — there is no hidden input. Tradeoff for portability; config file gets `0o600` afterwards.
- `Provider::OpenaiCompat` works with any OpenAI-compatible service (DeepSeek, Moonshot, Groq, vLLM, etc.). DeepSeek v4-flash's prefix cache hits the static `system + candidate listing` segment of every call — `Oneshot` mode benefits most because the only thing that changes turn-to-turn is the trailing user prompt. `Conversation` mode invalidates the trailing cache as the assistant history grows, paying for the recall improvement with tokens.
- Setup wizard reads stdin line-by-line; piping `runai recommend setup < answers.txt` works for automation.
