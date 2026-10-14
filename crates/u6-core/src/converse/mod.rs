//! Conversations: Talk 0x1127c4, the script interpreter 0x1130a2, script and portrait loading
//! 0x133e5e / 0x133d3e (WU-P6).
//!
//! Domain state = [`ConverseState`] (interpreter state; empty until this module is ported).
//!
//! Original functions this module will own (milestone 1):
//!
//! - 0x1125b8 [converse] converse text substitutions (milord/milady, time of day)
//! - 0x1127c4 [converse] Talk command: target, shrine/statue cases, start talk
//! - 0x1128d0 [converse] unnamed; 1 callees
//! - 0x112b0a [converse] leaf; callers 2, from 1 subsystems
//! - 0x112b20 [converse] uses "Undefined."
//! - 0x112ba4 [converse] unnamed; 1 callees
//! - 0x112bf2 [converse] leaf; callers 7, from 2 subsystems
//! - 0x112d72 [converse] step of 0x114afa (start conversation: checks, load ~)
//! - 0x112daa [converse] unnamed; 2 callees
//! - 0x112eb8 [converse] step of 0x1130a2 (converse interpreter main loop (i~)
//! - 0x11304a [converse] unnamed; 2 callees
//! - 0x1130a2 [converse] converse interpreter main loop (input, keyword match)
//! - 0x113f8a [converse] step of 0x11404c (converse opcode executor)
//! - 0x11404c [converse] converse opcode executor
//! - 0x1148f0 [converse] print converse text, control chars
//! - 0x1149d6 [converse] step of 0x1148f0 (print converse text, control chars)
//! - 0x114afa [converse] start conversation: checks, load script, greeting
//! - 0x11519e [converse] step of 0x1130a2 (converse interpreter main loop (i~)
//! - 0x133d3e [converse] load an NPC portrait (portrait.a)
//! - 0x133e5e [converse] load an NPC's converse.a script block

/// Conversation interpreter state.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConverseState {}
