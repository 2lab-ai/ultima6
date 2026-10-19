//! The op tables the disassembler decodes with, read off the interpreter's arms.
//!
//! - Statement ops: the arms of the executor (`exec.rs` `exec`), with the operands each arm
//!   reads. Every byte 0x80..=0xee that is not `ee` / `ef` and has no row here falls into
//!   `exec`'s default arm (the unknown-command line); the disassembler refuses it.
//! - Expression ops: the arms of the evaluator (`eval.rs` `apply`), with the stack slots each
//!   arm reduces. In an expression every byte other than the literal prefixes `d2` / `d3` / `d4`
//!   and the terminators `a7` / `a8` is one token, an op of this table or a pushed value, so
//!   this table only names tokens; it never changes how many bytes an expression takes.
//! - Escapes: the characters the escape layer (`ui/printf.rs` `text_out`) acts on.

/// How a statement op's operands are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// Expressions in order, each up to its `a7` / `a8`; an [`Arg::Actor`] is an expression the
    /// executor then resolves to an actor (0xeb = the NPC talked to).
    Args(&'static [Arg]),
    /// `b0`: a 32-bit little-endian script address; the low word is the new read position.
    Jump,
    /// `b5`: `d2 <table>` (a table string), `d5` (nothing) or `<kind> <slot> b3 <byte>`.
    Print,
    /// `a6`: `d2 <table> (b1 a8 | <index ending a8>) <expr>` (a table word), `<var> b2 a8
    /// <expr>` (a word variable), `<var> <kind> a8 d2 <table> (b1 | <index>)` or `<var> <kind>
    /// a8 <slot> b3` (a string slot).
    Assign,
}

/// One expression operand of [`Shape::Args`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    Expr,
    Actor,
}

/// What the interpreter does after the op, for the disassembler's control flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    /// The next byte.
    Next,
    /// `a1`: the next byte when the condition holds, else past the first `a3` / `a2` a scan
    /// finds (stepping over `b0` / `d3` / `d4` operands).
    If,
    /// `a3`: past the first `a2` the same scan finds.
    Else,
    /// `b0`: the operand's address.
    Jump,
    /// `b6`: the conversation ends after it.
    Bye,
}

/// One statement op: its byte, a name for its role, its operands and its control flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatementOp {
    pub code: u8,
    pub name: &'static str,
    pub shape: Shape,
    pub flow: Flow,
}

use Arg::{Actor, Expr};

const fn op(code: u8, name: &'static str, args: &'static [Arg]) -> StatementOp {
    StatementOp {
        code,
        name,
        shape: Shape::Args(args),
        flow: Flow::Next,
    }
}

const fn flow(code: u8, name: &'static str, shape: Shape, flow: Flow) -> StatementOp {
    StatementOp {
        code,
        name,
        shape,
        flow,
    }
}

/// The executor's arms: 28 op bytes in 24 arms (`a2`/`a7`, `a4`/`a5`, `b9`/`ba` and `d8`/`df`
/// each share one).
pub const STATEMENT_OPS: [StatementOp; 28] = [
    op(0x9c, "mount", &[Actor]),
    op(0x9e, "sleep_until_five", &[]),
    flow(0xa1, "if", Shape::Args(&[Expr]), Flow::If),
    op(0xa2, "endif", &[]),
    flow(0xa3, "else", Shape::Args(&[]), Flow::Else),
    op(0xa4, "set_talk_flag", &[Actor, Expr]),
    op(0xa5, "clear_talk_flag", &[Actor, Expr]),
    flow(0xa6, "assign", Shape::Assign, Flow::Next),
    op(0xa7, "nop", &[]),
    flow(0xb0, "jump", Shape::Jump, Flow::Jump),
    flow(0xb5, "print", Shape::Print, Flow::Next),
    flow(0xb6, "bye", Shape::Args(&[]), Flow::Bye),
    op(0xb9, "add_object", &[Actor, Expr, Expr, Expr]),
    op(0xba, "remove_object", &[Actor, Expr, Expr, Expr]),
    op(0xbe, "inventory", &[Expr]),
    op(0xbf, "portrait", &[Actor]),
    op(0xc4, "add_karma", &[Expr]),
    op(0xc5, "sub_karma", &[Expr]),
    op(0xc8, "move_into", &[Actor, Expr]),
    op(0xc9, "move_object", &[Expr, Expr, Actor, Actor]),
    op(0xcb, "wait_key", &[]),
    op(0xcd, "set_worktype", &[Actor, Expr]),
    op(0xd0, "refresh_view", &[Expr]),
    op(0xd6, "resurrect", &[Actor]),
    op(0xd8, "actor_name", &[Actor]),
    op(0xd9, "heal", &[Actor]),
    op(0xdb, "cure", &[Actor]),
    op(0xdf, "actor_name_or_you", &[Actor]),
];

/// The statement op `code`, or `None` for a byte the executor's default arm takes.
pub fn statement_op(code: u8) -> Option<&'static StatementOp> {
    STATEMENT_OPS.iter().find(|o| o.code == code)
}

/// One expression op: its byte, a name for its role and the stack slots it takes (the value
/// replaces them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExprOp {
    pub code: u8,
    pub name: &'static str,
    pub arity: u8,
}

const fn e(code: u8, name: &'static str, arity: u8) -> ExprOp {
    ExprOp { code, name, arity }
}

/// The evaluator's op arms: 42 op bytes in 39 arms (`bc`/`bd` share one, `e2`..`e4` another).
pub const EXPR_OPS: [ExprOp; 42] = [
    e(0x81, "gt", 2),
    e(0x82, "ge", 2),
    e(0x83, "lt", 2),
    e(0x84, "le", 2),
    e(0x85, "ne", 2),
    e(0x86, "eq", 2),
    e(0x90, "add", 2),
    e(0x91, "sub", 2),
    e(0x92, "mul", 2),
    e(0x93, "div", 2),
    e(0x94, "or", 2),
    e(0x95, "and", 2),
    e(0x9a, "can_carry", 1),
    e(0x9b, "weight", 2),
    e(0x9d, "is_riding", 1),
    e(0x9f, "has_object", 3),
    e(0xa0, "random", 2),
    e(0xab, "talk_flag", 2),
    e(0xb2, "int_var", 1),
    e(0xb3, "string_var", 1),
    e(0xb4, "table_word", 2),
    e(0xb7, "index_of_string", 2),
    e(0xbb, "object_count", 2),
    e(0xbc, "hundred", 2),
    e(0xbd, "hundred_too", 2),
    e(0xc0, "select_item", 0),
    e(0xc1, "location", 1),
    e(0xc2, "object_type", 1),
    e(0xc3, "index_of_word", 2),
    e(0xc6, "in_party_here", 1),
    e(0xc7, "member_with_object", 2),
    e(0xca, "join_party", 1),
    e(0xcc, "leave_party", 1),
    e(0xd7, "on_screen", 1),
    e(0xda, "wounded", 1),
    e(0xdc, "poisoned", 1),
    e(0xdd, "party_member", 2),
    e(0xe0, "add_exp", 2),
    e(0xe1, "add_level", 2),
    e(0xe2, "add_strength", 2),
    e(0xe3, "add_intelligence", 2),
    e(0xe4, "add_dexterity", 2),
];

/// The expression op `code`, or `None` for a byte the evaluator pushes as a value.
pub fn expr_op(code: u8) -> Option<&'static ExprOp> {
    EXPR_OPS.iter().find(|o| o.code == code)
}

/// Expression literal prefixes: `d2` 32-bit, `d3` 8-bit, `d4` 16-bit, little-endian.
pub const LIT32: u8 = 0xd2;
pub const LIT8: u8 = 0xd3;
pub const LIT16: u8 = 0xd4;
/// Expression terminators (both consumed).
pub const END: u8 = 0xa7;
pub const END_ASSIGN: u8 = 0xa8;

/// Script structure bytes.
pub const HEADER: u8 = 0xff;
pub const LOOK: u8 = 0xf1;
pub const GREETING: u8 = 0xf2;
pub const KEYWORDS: u8 = 0xef;
pub const ANSWER: u8 = 0xf6;
pub const BLOCK_END: u8 = 0xee;
/// `b1` after a table address: the table's first element, no index expression.
pub const TABLE_FIRST: u8 = 0xb1;
/// Ends the tables `b7` and `c3` search (at an entry's start).
pub const TABLE_END: u8 = 0xb8;
/// String-slot and word-variable tags.
pub const STR_TAG: u8 = 0xb3;
pub const INT_TAG: u8 = 0xb2;
/// `b5`'s "print nothing" form.
pub const PRINT_NOTHING: u8 = 0xd5;

/// A byte `run_text` prints (0x20..0x7f and newline).
pub fn is_text(b: u8) -> bool {
    (0x20..0x80).contains(&b) || b == b'\n'
}

/// The operand bytes the skip loops step over after a byte: the interpreter's own
/// `run.rs` `operand_len` (4 after `b0`, 1 after `d3`, 2 after `d4`, else none).
pub fn skip_len(b: u8) -> u32 {
    super::super::run::operand_len(b) as u32
}

/// An escape of the escape layer inside a text run (`ui/printf.rs` `text_out`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Escape {
    /// `$n`: string variable `n` (0..35, as `var_index` reads the letter); `None` when no
    /// ordinary character follows inside the run (the prefix stays pending).
    Str(Option<u8>),
    /// `#n`: word variable `n` in decimal; also decides singular / plural.
    Int(Option<u8>),
    /// `^c`: control glyph `c & 0x1f`.
    Glyph(Option<u8>),
    /// `*`: wait for a key.
    Wait,
    /// `@`: a keyword in the alert colour, up to a keyword-end character.
    Keyword,
    /// `&`: hidden text on / off.
    Hidden,
    /// `<` / `>`: runic glyphs on / off.
    RunicOn,
    RunicOff,
    /// A back-quote: capitalise the next character.
    Capital,
    /// `/` and `\`: the letters that follow print only for a singular / a plural.
    Singular,
    Plural,
    /// `%`: the following characters print unescaped up to the next prefix.
    Literal,
}

impl Escape {
    /// The escape a byte starts, or `None` for an ordinary character. Prefixes that take the
    /// next character (`$ # ^`) come back with `None` for it; the caller fills it in.
    pub fn start(b: u8) -> Option<Escape> {
        Some(match b {
            b'$' => Escape::Str(None),
            b'#' => Escape::Int(None),
            b'^' => Escape::Glyph(None),
            b'*' => Escape::Wait,
            b'@' => Escape::Keyword,
            b'&' => Escape::Hidden,
            b'<' => Escape::RunicOn,
            b'>' => Escape::RunicOff,
            b'`' => Escape::Capital,
            b'/' => Escape::Singular,
            b'\\' => Escape::Plural,
            b'%' => Escape::Literal,
            _ => return None,
        })
    }

    /// The prefixes that take the next ordinary character as their argument.
    pub fn takes_arg(self) -> bool {
        matches!(self, Escape::Str(_) | Escape::Int(_) | Escape::Glyph(_))
    }
}

/// `$n` / `#n` index of the escape layer: digits 0..9, letters (either case) 10..35, any other
/// byte 0.
pub fn var_index(c: u8) -> u8 {
    match c.to_ascii_uppercase() {
        d @ b'0'..=b'9' => d - b'0',
        l @ b'A'..=b'Z' => l - b'A' + 10,
        _ => 0,
    }
}
