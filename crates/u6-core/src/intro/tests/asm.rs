//! 68000 instruction words for the synthetic `u` images: the encodings of the instruction forms
//! [`crate::intro::passages`] reads (Motorola's operation-word layout), named so a test builds
//! its code from instructions and its own addresses, not from a copy of `u`'s bytes.

/// `move.l d7,d0`.
pub(super) const MOVE_L_D7_D0: u16 = 0x2007;
/// `move.l d6,d0`.
pub(super) const MOVE_L_D6_D0: u16 = 0x2006;
/// `tst.w d0`.
pub(super) const TST_W_D0: u16 = 0x4a40;
/// `beq.w <d16>` (the displacement word follows).
pub(super) const BEQ_W: u16 = 0x6700;
/// `bra.w <d16>` (the displacement word follows).
pub(super) const BRA_W: u16 = 0x6000;
/// `move.w d0,<d16>(a4)`.
pub(super) const MOVE_W_D0_TO_A4: u16 = 0x3940;
/// `move.w #<imm>,<d16>(a4)` (immediate word, then displacement word).
pub(super) const MOVE_W_IMM_TO_A4: u16 = 0x397c;
/// `move.w <d16>(a4),d0`.
pub(super) const MOVE_W_A4_TO_D0: u16 = 0x302c;
/// `lea <d16>(a4),a0`.
pub(super) const LEA_A4_A0: u16 = 0x41ec;
/// `move.l (0,a0,d0.l),(a7)`, with its brief extension word.
pub(super) const MOVE_L_A0_D0_TO_SP: [u16; 2] = [0x2eb0, 0x0800];
/// `jsr <d16>(pc)`.
pub(super) const JSR_PC: u16 = 0x4eba;
/// `pea <d16>(pc)`.
pub(super) const PEA_PC: u16 = 0x487a;
/// `subq.w #2,a7`.
pub(super) const SUBQ_W_2_A7: u16 = 0x554f;
/// `addq.w #8,a7`.
pub(super) const ADDQ_W_8_A7: u16 = 0x504f;
/// `move.w d0,-(a7)`.
pub(super) const PUSH_W_D0: u16 = 0x3f00;
/// `lea <d16>(a7),a7`.
pub(super) const LEA_A7_A7: u16 = 0x4fef;
/// `move.b #<imm>,<d16>(a5)` (immediate word, then displacement word).
pub(super) const MOVE_B_IMM_TO_A5: u16 = 0x1b7c;
/// `movea.l <d16>(a5),a0`.
pub(super) const MOVEA_L_A5_A0: u16 = 0x206d;

/// `moveq #n,d0`.
pub(super) const fn moveq_d0(n: i8) -> u16 {
    0x7000 | n as u8 as u16
}

/// `jmp <addr>.l`: the operation word and the address, high word first.
pub(super) const fn jmp_abs(addr: u32) -> [u16; 3] {
    [0x4ef9, (addr >> 16) as u16, addr as u16]
}
