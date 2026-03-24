pub mod registers;
use registers::Registers;
use crate::mmu::Mmu;

/// Interrupt flag bits
pub const INT_VBLANK: u8  = 0x01;
pub const INT_STAT: u8    = 0x02;
pub const INT_TIMER: u8   = 0x04;
pub const INT_SERIAL: u8  = 0x08;
pub const INT_JOYPAD: u8  = 0x10;

/// Interrupt vectors
const INT_VBLANK_VEC: u16  = 0x0040;
const INT_STAT_VEC: u16    = 0x0048;
const INT_TIMER_VEC: u16   = 0x0050;
const INT_SERIAL_VEC: u16  = 0x0058;
const INT_JOYPAD_VEC: u16  = 0x0060;

#[derive(Clone)]
pub struct Cpu {
    pub regs: Registers,
    pub ime: bool,        // Interrupt Master Enable
    ime_scheduled: bool, // EI schedules IME one instruction later
    pub halted: bool,
    pub stopped: bool,
    /// Accumulated T-cycle counter for the last step
    pub cycles: u32,
}

impl Cpu {
    pub fn new(cgb_mode: bool) -> Self {
        Self {
            regs: Registers::new(cgb_mode),
            ime: false,
            ime_scheduled: false,
            halted: false,
            stopped: false,
            cycles: 0,
        }
    }

    /// Execute one instruction (or handle interrupt). Returns T-cycles consumed.
    pub fn step(&mut self, mmu: &mut Mmu) -> u32 {
        self.cycles = 0;

        // Handle pending interrupts
        if self.handle_interrupts(mmu) {
            return self.cycles;
        }

        // Enable IME after EI delay
        if self.ime_scheduled {
            self.ime_scheduled = false;
            self.ime = true;
        }

        if self.halted {
            // HALT: 4 cycles per iteration, wake up on interrupt request
            let ie = mmu.read_byte(0xFFFF);
            let iflags = mmu.read_byte(0xFF0F);
            if ie & iflags & 0x1F != 0 {
                self.halted = false;
            }
            self.tick(mmu);
            return self.cycles;
        }

        let opcode = self.fetch_byte(mmu);
        self.execute(mmu, opcode);
        self.cycles
    }

    fn handle_interrupts(&mut self, mmu: &mut Mmu) -> bool {
        let ie = mmu.read_byte(0xFFFF);
        let iflags = mmu.read_byte(0xFF0F);
        let triggered = ie & iflags & 0x1F;

        if triggered == 0 { return false; }

        // Wake from HALT regardless of IME
        if self.halted {
            self.halted = false;
        }

        if !self.ime { return false; }

        self.ime = false;

        // Service highest priority interrupt
        let (bit, vector) = if triggered & INT_VBLANK != 0 {
            (INT_VBLANK, INT_VBLANK_VEC)
        } else if triggered & INT_STAT != 0 {
            (INT_STAT, INT_STAT_VEC)
        } else if triggered & INT_TIMER != 0 {
            (INT_TIMER, INT_TIMER_VEC)
        } else if triggered & INT_SERIAL != 0 {
            (INT_SERIAL, INT_SERIAL_VEC)
        } else {
            (INT_JOYPAD, INT_JOYPAD_VEC)
        };

        // Acknowledge interrupt
        let iflags = mmu.read_byte(0xFF0F) & !bit;
        mmu.write_byte(0xFF0F, iflags);

        // Two wait cycles + push PC + jump
        self.tick(mmu);
        self.tick(mmu);
        let pc = self.regs.pc;
        self.push_u16(mmu, pc);
        self.regs.pc = vector;
        self.tick(mmu);

        true
    }

    // ─── Memory access helpers ────────────────────────────────────────────

    fn tick(&mut self, mmu: &mut Mmu) {
        self.cycles += 4;
        mmu.tick(4);
    }

    fn fetch_byte(&mut self, mmu: &mut Mmu) -> u8 {
        let v = mmu.read_byte(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        self.tick(mmu);
        v
    }

    fn fetch_word(&mut self, mmu: &mut Mmu) -> u16 {
        let lo = self.fetch_byte(mmu) as u16;
        let hi = self.fetch_byte(mmu) as u16;
        (hi << 8) | lo
    }

    fn read_byte(&mut self, mmu: &mut Mmu, addr: u16) -> u8 {
        let v = mmu.read_byte(addr);
        self.tick(mmu);
        v
    }

    fn write_byte(&mut self, mmu: &mut Mmu, addr: u16, val: u8) {
        mmu.write_byte(addr, val);
        self.tick(mmu);
    }

    fn push_u16(&mut self, mmu: &mut Mmu, val: u16) {
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write_byte(mmu, self.regs.sp, (val >> 8) as u8);
        self.regs.sp = self.regs.sp.wrapping_sub(1);
        self.write_byte(mmu, self.regs.sp, val as u8);
    }

    fn pop_u16(&mut self, mmu: &mut Mmu) -> u16 {
        let lo = self.read_byte(mmu, self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        let hi = self.read_byte(mmu, self.regs.sp) as u16;
        self.regs.sp = self.regs.sp.wrapping_add(1);
        (hi << 8) | lo
    }

    // ─── Register helpers ─────────────────────────────────────────────────

    fn get_r8(&self, mmu: &Mmu, r: u8) -> u8 {
        match r {
            0 => self.regs.b,
            1 => self.regs.c,
            2 => self.regs.d,
            3 => self.regs.e,
            4 => self.regs.h,
            5 => self.regs.l,
            6 => mmu.read_byte(self.regs.hl()), // (HL) - note: NO tick here, caller must tick
            7 => self.regs.a,
            _ => unreachable!(),
        }
    }

    fn set_r8(&mut self, mmu: &mut Mmu, r: u8, val: u8) {
        match r {
            0 => self.regs.b = val,
            1 => self.regs.c = val,
            2 => self.regs.d = val,
            3 => self.regs.e = val,
            4 => self.regs.h = val,
            5 => self.regs.l = val,
            6 => { mmu.write_byte(self.regs.hl(), val); } // caller ticks
            7 => self.regs.a = val,
            _ => unreachable!(),
        }
    }

    // ─── ALU operations ───────────────────────────────────────────────────

    fn alu_add(&mut self, val: u8, carry: bool) {
        let c = carry as u8;
        let result = self.regs.a.wrapping_add(val).wrapping_add(c);
        let half = (self.regs.a & 0x0F) + (val & 0x0F) + c > 0x0F;
        let full_carry = (self.regs.a as u16) + (val as u16) + (c as u16) > 0xFF;
        self.regs.set_flags(result == 0, false, half, full_carry);
        self.regs.a = result;
    }

    fn alu_sub(&mut self, val: u8, carry: bool, store: bool) {
        let c = carry as u8;
        let result = self.regs.a.wrapping_sub(val).wrapping_sub(c);
        let half = (self.regs.a & 0x0F) < (val & 0x0F) + c;
        let full_carry = (self.regs.a as u16) < (val as u16) + (c as u16);
        self.regs.set_flags(result == 0, true, half, full_carry);
        if store { self.regs.a = result; }
    }

    fn alu_and(&mut self, val: u8) {
        self.regs.a &= val;
        self.regs.set_flags(self.regs.a == 0, false, true, false);
    }

    fn alu_xor(&mut self, val: u8) {
        self.regs.a ^= val;
        self.regs.set_flags(self.regs.a == 0, false, false, false);
    }

    fn alu_or(&mut self, val: u8) {
        self.regs.a |= val;
        self.regs.set_flags(self.regs.a == 0, false, false, false);
    }

    fn alu_cp(&mut self, val: u8) {
        self.alu_sub(val, false, false);
    }

    fn alu_inc(&mut self, val: u8) -> u8 {
        let result = val.wrapping_add(1);
        self.regs.set_z(result == 0);
        self.regs.set_n(false);
        self.regs.set_h((val & 0x0F) == 0x0F);
        result
    }

    fn alu_dec(&mut self, val: u8) -> u8 {
        let result = val.wrapping_sub(1);
        self.regs.set_z(result == 0);
        self.regs.set_n(true);
        self.regs.set_h((val & 0x0F) == 0x00);
        result
    }

    fn alu_rlc(&mut self, val: u8) -> u8 {
        let c = (val >> 7) & 1;
        let result = (val << 1) | c;
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_rrc(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = (val >> 1) | (c << 7);
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_rl(&mut self, val: u8) -> u8 {
        let old_c = self.regs.flag_c() as u8;
        let c = (val >> 7) & 1;
        let result = (val << 1) | old_c;
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_rr(&mut self, val: u8) -> u8 {
        let old_c = self.regs.flag_c() as u8;
        let c = val & 1;
        let result = (val >> 1) | (old_c << 7);
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_sla(&mut self, val: u8) -> u8 {
        let c = (val >> 7) & 1;
        let result = val << 1;
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_sra(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = ((val as i8) >> 1) as u8;
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_srl(&mut self, val: u8) -> u8 {
        let c = val & 1;
        let result = val >> 1;
        self.regs.set_flags(result == 0, false, false, c != 0);
        result
    }

    fn alu_swap(&mut self, val: u8) -> u8 {
        let result = (val >> 4) | (val << 4);
        self.regs.set_flags(result == 0, false, false, false);
        result
    }

    fn alu_bit(&mut self, bit: u8, val: u8) {
        self.regs.set_z((val >> bit) & 1 == 0);
        self.regs.set_n(false);
        self.regs.set_h(true);
    }

    // ─── Instruction execution ────────────────────────────────────────────

    fn execute(&mut self, mmu: &mut Mmu, opcode: u8) {
        match opcode {
            // ── NOP ──────────────────────────────────────────────────────
            0x00 => {}

            // ── LD r16, u16 ──────────────────────────────────────────────
            0x01 => { let v = self.fetch_word(mmu); self.regs.set_bc(v); }
            0x11 => { let v = self.fetch_word(mmu); self.regs.set_de(v); }
            0x21 => { let v = self.fetch_word(mmu); self.regs.set_hl(v); }
            0x31 => { let v = self.fetch_word(mmu); self.regs.sp = v; }

            // ── LD (r16), A ───────────────────────────────────────────────
            0x02 => { let a = self.regs.a; let addr = self.regs.bc(); self.write_byte(mmu, addr, a); }
            0x12 => { let a = self.regs.a; let addr = self.regs.de(); self.write_byte(mmu, addr, a); }
            0x22 => {
                let a = self.regs.a;
                let hl = self.regs.hl();
                self.write_byte(mmu, hl, a);
                self.regs.set_hl(hl.wrapping_add(1));
            }
            0x32 => {
                let a = self.regs.a;
                let hl = self.regs.hl();
                self.write_byte(mmu, hl, a);
                self.regs.set_hl(hl.wrapping_sub(1));
            }

            // ── INC r16 ───────────────────────────────────────────────────
            0x03 => { let v = self.regs.bc().wrapping_add(1); self.regs.set_bc(v); self.tick(mmu); }
            0x13 => { let v = self.regs.de().wrapping_add(1); self.regs.set_de(v); self.tick(mmu); }
            0x23 => { let v = self.regs.hl().wrapping_add(1); self.regs.set_hl(v); self.tick(mmu); }
            0x33 => { self.regs.sp = self.regs.sp.wrapping_add(1); self.tick(mmu); }

            // ── INC r8 ────────────────────────────────────────────────────
            0x04 => { let v = self.alu_inc(self.regs.b); self.regs.b = v; }
            0x0C => { let v = self.alu_inc(self.regs.c); self.regs.c = v; }
            0x14 => { let v = self.alu_inc(self.regs.d); self.regs.d = v; }
            0x1C => { let v = self.alu_inc(self.regs.e); self.regs.e = v; }
            0x24 => { let v = self.alu_inc(self.regs.h); self.regs.h = v; }
            0x2C => { let v = self.alu_inc(self.regs.l); self.regs.l = v; }
            0x34 => {
                let hl = self.regs.hl();
                let v = self.read_byte(mmu, hl);
                let r = self.alu_inc(v);
                self.write_byte(mmu, hl, r);
            }
            0x3C => { let v = self.alu_inc(self.regs.a); self.regs.a = v; }

            // ── DEC r8 ────────────────────────────────────────────────────
            0x05 => { let v = self.alu_dec(self.regs.b); self.regs.b = v; }
            0x0D => { let v = self.alu_dec(self.regs.c); self.regs.c = v; }
            0x15 => { let v = self.alu_dec(self.regs.d); self.regs.d = v; }
            0x1D => { let v = self.alu_dec(self.regs.e); self.regs.e = v; }
            0x25 => { let v = self.alu_dec(self.regs.h); self.regs.h = v; }
            0x2D => { let v = self.alu_dec(self.regs.l); self.regs.l = v; }
            0x35 => {
                let hl = self.regs.hl();
                let v = self.read_byte(mmu, hl);
                let r = self.alu_dec(v);
                self.write_byte(mmu, hl, r);
            }
            0x3D => { let v = self.alu_dec(self.regs.a); self.regs.a = v; }

            // ── LD r8, u8 ─────────────────────────────────────────────────
            0x06 => { self.regs.b = self.fetch_byte(mmu); }
            0x0E => { self.regs.c = self.fetch_byte(mmu); }
            0x16 => { self.regs.d = self.fetch_byte(mmu); }
            0x1E => { self.regs.e = self.fetch_byte(mmu); }
            0x26 => { self.regs.h = self.fetch_byte(mmu); }
            0x2E => { self.regs.l = self.fetch_byte(mmu); }
            0x36 => { let v = self.fetch_byte(mmu); let hl = self.regs.hl(); self.write_byte(mmu, hl, v); }
            0x3E => { self.regs.a = self.fetch_byte(mmu); }

            // ── Rotates (A register, no Z flag) ───────────────────────────
            0x07 => {
                let c = self.regs.a >> 7;
                self.regs.a = (self.regs.a << 1) | c;
                self.regs.set_flags(false, false, false, c != 0);
            }
            0x0F => {
                let c = self.regs.a & 1;
                self.regs.a = (self.regs.a >> 1) | (c << 7);
                self.regs.set_flags(false, false, false, c != 0);
            }
            0x17 => {
                let old_c = self.regs.flag_c() as u8;
                let c = self.regs.a >> 7;
                self.regs.a = (self.regs.a << 1) | old_c;
                self.regs.set_flags(false, false, false, c != 0);
            }
            0x1F => {
                let old_c = self.regs.flag_c() as u8;
                let c = self.regs.a & 1;
                self.regs.a = (self.regs.a >> 1) | (old_c << 7);
                self.regs.set_flags(false, false, false, c != 0);
            }

            // ── LD (u16), SP ──────────────────────────────────────────────
            0x08 => {
                let addr = self.fetch_word(mmu);
                self.write_byte(mmu, addr, self.regs.sp as u8);
                self.write_byte(mmu, addr.wrapping_add(1), (self.regs.sp >> 8) as u8);
            }

            // ── ADD HL, r16 ───────────────────────────────────────────────
            0x09 => {
                let hl = self.regs.hl(); let bc = self.regs.bc();
                let r = hl.wrapping_add(bc);
                self.regs.set_n(false);
                self.regs.set_h((hl & 0xFFF) + (bc & 0xFFF) > 0xFFF);
                self.regs.set_c((hl as u32) + (bc as u32) > 0xFFFF);
                self.regs.set_hl(r);
                self.tick(mmu);
            }
            0x19 => {
                let hl = self.regs.hl(); let de = self.regs.de();
                let r = hl.wrapping_add(de);
                self.regs.set_n(false);
                self.regs.set_h((hl & 0xFFF) + (de & 0xFFF) > 0xFFF);
                self.regs.set_c((hl as u32) + (de as u32) > 0xFFFF);
                self.regs.set_hl(r);
                self.tick(mmu);
            }
            0x29 => {
                let hl = self.regs.hl();
                let r = hl.wrapping_add(hl);
                self.regs.set_n(false);
                self.regs.set_h((hl & 0xFFF) + (hl & 0xFFF) > 0xFFF);
                self.regs.set_c((hl as u32) + (hl as u32) > 0xFFFF);
                self.regs.set_hl(r);
                self.tick(mmu);
            }
            0x39 => {
                let hl = self.regs.hl(); let sp = self.regs.sp;
                let r = hl.wrapping_add(sp);
                self.regs.set_n(false);
                self.regs.set_h((hl & 0xFFF) + (sp & 0xFFF) > 0xFFF);
                self.regs.set_c((hl as u32) + (sp as u32) > 0xFFFF);
                self.regs.set_hl(r);
                self.tick(mmu);
            }

            // ── LD A, (r16) ───────────────────────────────────────────────
            0x0A => { let addr = self.regs.bc(); self.regs.a = self.read_byte(mmu, addr); }
            0x1A => { let addr = self.regs.de(); self.regs.a = self.read_byte(mmu, addr); }
            0x2A => {
                let hl = self.regs.hl();
                self.regs.a = self.read_byte(mmu, hl);
                self.regs.set_hl(hl.wrapping_add(1));
            }
            0x3A => {
                let hl = self.regs.hl();
                self.regs.a = self.read_byte(mmu, hl);
                self.regs.set_hl(hl.wrapping_sub(1));
            }

            // ── DEC r16 ───────────────────────────────────────────────────
            0x0B => { let v = self.regs.bc().wrapping_sub(1); self.regs.set_bc(v); self.tick(mmu); }
            0x1B => { let v = self.regs.de().wrapping_sub(1); self.regs.set_de(v); self.tick(mmu); }
            0x2B => { let v = self.regs.hl().wrapping_sub(1); self.regs.set_hl(v); self.tick(mmu); }
            0x3B => { self.regs.sp = self.regs.sp.wrapping_sub(1); self.tick(mmu); }

            // ── DAA ───────────────────────────────────────────────────────
            0x27 => {
                let mut a = self.regs.a;
                if !self.regs.flag_n() {
                    if self.regs.flag_c() || a > 0x99 { a = a.wrapping_add(0x60); self.regs.set_c(true); }
                    if self.regs.flag_h() || (a & 0x0F) > 0x09 { a = a.wrapping_add(0x06); }
                } else {
                    if self.regs.flag_c() { a = a.wrapping_sub(0x60); }
                    if self.regs.flag_h() { a = a.wrapping_sub(0x06); }
                }
                self.regs.set_z(a == 0);
                self.regs.set_h(false);
                self.regs.a = a;
            }

            // ── CPL ───────────────────────────────────────────────────────
            0x2F => {
                self.regs.a = !self.regs.a;
                self.regs.set_n(true);
                self.regs.set_h(true);
            }

            // ── SCF / CCF ─────────────────────────────────────────────────
            0x37 => { self.regs.set_n(false); self.regs.set_h(false); self.regs.set_c(true); }
            0x3F => {
                let c = self.regs.flag_c();
                self.regs.set_n(false);
                self.regs.set_h(false);
                self.regs.set_c(!c);
            }

            // ── JR ───────────────────────────────────────────────────────
            0x18 => {
                let offset = self.fetch_byte(mmu) as i8 as i16;
                self.regs.pc = self.regs.pc.wrapping_add(offset as u16);
                self.tick(mmu);
            }
            0x20 => {
                let offset = self.fetch_byte(mmu) as i8 as i16;
                if !self.regs.flag_z() { self.regs.pc = self.regs.pc.wrapping_add(offset as u16); self.tick(mmu); }
            }
            0x28 => {
                let offset = self.fetch_byte(mmu) as i8 as i16;
                if self.regs.flag_z() { self.regs.pc = self.regs.pc.wrapping_add(offset as u16); self.tick(mmu); }
            }
            0x30 => {
                let offset = self.fetch_byte(mmu) as i8 as i16;
                if !self.regs.flag_c() { self.regs.pc = self.regs.pc.wrapping_add(offset as u16); self.tick(mmu); }
            }
            0x38 => {
                let offset = self.fetch_byte(mmu) as i8 as i16;
                if self.regs.flag_c() { self.regs.pc = self.regs.pc.wrapping_add(offset as u16); self.tick(mmu); }
            }

            // ── LD r8, r8 block (0x40–0x7F) ──────────────────────────────
            0x40..=0x7F => {
                if opcode == 0x76 {
                    // HALT
                    self.halted = true;
                } else {
                    let src_r = opcode & 0x07;
                    let dst_r = (opcode >> 3) & 0x07;
                    let val = if src_r == 6 {
                        let addr = self.regs.hl();
                        self.read_byte(mmu, addr)
                    } else {
                        self.get_r8(mmu, src_r)
                    };
                    if dst_r == 6 {
                        let addr = self.regs.hl();
                        self.write_byte(mmu, addr, val);
                    } else {
                        self.set_r8(mmu, dst_r, val);
                    }
                }
            }

            // ── ALU block (0x80–0xBF) ─────────────────────────────────────
            0x80..=0xBF => {
                let r = opcode & 0x07;
                let val = if r == 6 {
                    let addr = self.regs.hl();
                    self.read_byte(mmu, addr)
                } else {
                    self.get_r8(mmu, r)
                };
                match (opcode >> 3) & 0x07 {
                    0 => self.alu_add(val, false),
                    1 => self.alu_add(val, self.regs.flag_c()),
                    2 => self.alu_sub(val, false, true),
                    3 => self.alu_sub(val, self.regs.flag_c(), true),
                    4 => self.alu_and(val),
                    5 => self.alu_xor(val),
                    6 => self.alu_or(val),
                    7 => self.alu_cp(val),
                    _ => unreachable!(),
                }
            }

            // ── ALU immediate (0xC6, 0xCE, 0xD6, 0xDE, 0xE6, 0xEE, 0xF6, 0xFE) ──
            0xC6 => { let v = self.fetch_byte(mmu); self.alu_add(v, false); }
            0xCE => { let v = self.fetch_byte(mmu); self.alu_add(v, self.regs.flag_c()); }
            0xD6 => { let v = self.fetch_byte(mmu); self.alu_sub(v, false, true); }
            0xDE => { let v = self.fetch_byte(mmu); self.alu_sub(v, self.regs.flag_c(), true); }
            0xE6 => { let v = self.fetch_byte(mmu); self.alu_and(v); }
            0xEE => { let v = self.fetch_byte(mmu); self.alu_xor(v); }
            0xF6 => { let v = self.fetch_byte(mmu); self.alu_or(v); }
            0xFE => { let v = self.fetch_byte(mmu); self.alu_cp(v); }

            // ── RET ───────────────────────────────────────────────────────
            0xC9 => {
                let addr = self.pop_u16(mmu);
                self.regs.pc = addr;
                self.tick(mmu);
            }
            0xD9 => {
                // RETI
                let addr = self.pop_u16(mmu);
                self.regs.pc = addr;
                self.ime = true;
                self.tick(mmu);
            }
            0xC0 => {
                self.tick(mmu);
                if !self.regs.flag_z() { let a = self.pop_u16(mmu); self.regs.pc = a; self.tick(mmu); }
            }
            0xC8 => {
                self.tick(mmu);
                if self.regs.flag_z() { let a = self.pop_u16(mmu); self.regs.pc = a; self.tick(mmu); }
            }
            0xD0 => {
                self.tick(mmu);
                if !self.regs.flag_c() { let a = self.pop_u16(mmu); self.regs.pc = a; self.tick(mmu); }
            }
            0xD8 => {
                self.tick(mmu);
                if self.regs.flag_c() { let a = self.pop_u16(mmu); self.regs.pc = a; self.tick(mmu); }
            }

            // ── JP ────────────────────────────────────────────────────────
            0xC3 => {
                let addr = self.fetch_word(mmu);
                self.regs.pc = addr;
                self.tick(mmu);
            }
            0xE9 => { self.regs.pc = self.regs.hl(); }
            0xC2 => {
                let addr = self.fetch_word(mmu);
                if !self.regs.flag_z() { self.regs.pc = addr; self.tick(mmu); }
            }
            0xCA => {
                let addr = self.fetch_word(mmu);
                if self.regs.flag_z() { self.regs.pc = addr; self.tick(mmu); }
            }
            0xD2 => {
                let addr = self.fetch_word(mmu);
                if !self.regs.flag_c() { self.regs.pc = addr; self.tick(mmu); }
            }
            0xDA => {
                let addr = self.fetch_word(mmu);
                if self.regs.flag_c() { self.regs.pc = addr; self.tick(mmu); }
            }

            // ── CALL ──────────────────────────────────────────────────────
            0xCD => {
                let addr = self.fetch_word(mmu);
                self.tick(mmu);
                let pc = self.regs.pc;
                self.push_u16(mmu, pc);
                self.regs.pc = addr;
            }
            0xC4 => {
                let addr = self.fetch_word(mmu);
                if !self.regs.flag_z() {
                    self.tick(mmu);
                    let pc = self.regs.pc;
                    self.push_u16(mmu, pc);
                    self.regs.pc = addr;
                }
            }
            0xCC => {
                let addr = self.fetch_word(mmu);
                if self.regs.flag_z() {
                    self.tick(mmu);
                    let pc = self.regs.pc;
                    self.push_u16(mmu, pc);
                    self.regs.pc = addr;
                }
            }
            0xD4 => {
                let addr = self.fetch_word(mmu);
                if !self.regs.flag_c() {
                    self.tick(mmu);
                    let pc = self.regs.pc;
                    self.push_u16(mmu, pc);
                    self.regs.pc = addr;
                }
            }
            0xDC => {
                let addr = self.fetch_word(mmu);
                if self.regs.flag_c() {
                    self.tick(mmu);
                    let pc = self.regs.pc;
                    self.push_u16(mmu, pc);
                    self.regs.pc = addr;
                }
            }

            // ── RST ───────────────────────────────────────────────────────
            0xC7 | 0xCF | 0xD7 | 0xDF | 0xE7 | 0xEF | 0xF7 | 0xFF => {
                let vector = (opcode & 0x38) as u16;
                self.tick(mmu);
                let pc = self.regs.pc;
                self.push_u16(mmu, pc);
                self.regs.pc = vector;
            }

            // ── PUSH / POP ────────────────────────────────────────────────
            0xC5 => { let v = self.regs.bc(); self.tick(mmu); self.push_u16(mmu, v); }
            0xD5 => { let v = self.regs.de(); self.tick(mmu); self.push_u16(mmu, v); }
            0xE5 => { let v = self.regs.hl(); self.tick(mmu); self.push_u16(mmu, v); }
            0xF5 => { let v = self.regs.af(); self.tick(mmu); self.push_u16(mmu, v); }
            0xC1 => { let v = self.pop_u16(mmu); self.regs.set_bc(v); }
            0xD1 => { let v = self.pop_u16(mmu); self.regs.set_de(v); }
            0xE1 => { let v = self.pop_u16(mmu); self.regs.set_hl(v); }
            0xF1 => { let v = self.pop_u16(mmu); self.regs.set_af(v); }

            // ── LDH (IO) ──────────────────────────────────────────────────
            0xE0 => {
                let offset = self.fetch_byte(mmu) as u16;
                let addr = 0xFF00 | offset;
                let a = self.regs.a;
                self.write_byte(mmu, addr, a);
            }
            0xF0 => {
                let offset = self.fetch_byte(mmu) as u16;
                let addr = 0xFF00 | offset;
                self.regs.a = self.read_byte(mmu, addr);
            }
            0xE2 => {
                let addr = 0xFF00 | (self.regs.c as u16);
                let a = self.regs.a;
                self.write_byte(mmu, addr, a);
            }
            0xF2 => {
                let addr = 0xFF00 | (self.regs.c as u16);
                self.regs.a = self.read_byte(mmu, addr);
            }

            // ── LD (u16), A / LD A, (u16) ─────────────────────────────────
            0xEA => {
                let addr = self.fetch_word(mmu);
                let a = self.regs.a;
                self.write_byte(mmu, addr, a);
            }
            0xFA => {
                let addr = self.fetch_word(mmu);
                self.regs.a = self.read_byte(mmu, addr);
            }

            // ── SP operations ─────────────────────────────────────────────
            0xE8 => {
                let offset = self.fetch_byte(mmu) as i8 as i32;
                let sp = self.regs.sp as i32;
                let result = sp.wrapping_add(offset) as u16;
                let h = (sp ^ offset ^ (result as i32)) & 0x10 != 0;
                let c = (sp ^ offset ^ (result as i32)) & 0x100 != 0;
                self.regs.set_flags(false, false, h, c);
                self.regs.sp = result;
                self.tick(mmu);
                self.tick(mmu);
            }
            0xF8 => {
                let offset = self.fetch_byte(mmu) as i8 as i32;
                let sp = self.regs.sp as i32;
                let result = sp.wrapping_add(offset) as u16;
                let h = (sp ^ offset ^ (result as i32)) & 0x10 != 0;
                let c = (sp ^ offset ^ (result as i32)) & 0x100 != 0;
                self.regs.set_flags(false, false, h, c);
                self.regs.set_hl(result);
                self.tick(mmu);
            }
            0xF9 => {
                self.regs.sp = self.regs.hl();
                self.tick(mmu);
            }

            // ── IME control ───────────────────────────────────────────────
            0xF3 => { self.ime = false; self.ime_scheduled = false; }
            0xFB => { self.ime_scheduled = true; }

            // ── STOP ──────────────────────────────────────────────────────
            0x10 => {
                let _next = self.fetch_byte(mmu); // consume the 0x00
                self.stopped = true;
            }

            // ── CB prefix ─────────────────────────────────────────────────
            0xCB => {
                let cb_op = self.fetch_byte(mmu);
                self.execute_cb(mmu, cb_op);
            }

            // ── Unused / illegal ──────────────────────────────────────────
            _ => {
                // Treat as NOP for robustness
            }
        }
    }

    fn execute_cb(&mut self, mmu: &mut Mmu, op: u8) {
        let r = op & 0x07;
        let is_hl = r == 6;

        let val = if is_hl {
            let addr = self.regs.hl();
            self.read_byte(mmu, addr)
        } else {
            self.get_r8(mmu, r)
        };

        let result = match op >> 3 {
            0 => self.alu_rlc(val),
            1 => self.alu_rrc(val),
            2 => self.alu_rl(val),
            3 => self.alu_rr(val),
            4 => self.alu_sla(val),
            5 => self.alu_sra(val),
            6 => self.alu_swap(val),
            7 => self.alu_srl(val),
            8..=15 => { // BIT 0..7
                let bit = (op >> 3) & 0x07;
                self.alu_bit(bit, val);
                return; // BIT doesn't write back
            }
            16..=23 => { // RES 0..7
                let bit = (op >> 3) & 0x07;
                val & !(1 << bit)
            }
            24..=31 => { // SET 0..7
                let bit = (op >> 3) & 0x07;
                val | (1 << bit)
            }
            _ => unreachable!(),
        };

        if is_hl {
            let addr = self.regs.hl();
            self.write_byte(mmu, addr, result);
        } else {
            self.set_r8(mmu, r, result);
        }
    }
}
