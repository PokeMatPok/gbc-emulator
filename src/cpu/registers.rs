/// SM83 (Sharp LR35902) CPU registers

#[derive(Clone, Debug)]
pub struct Registers {
    pub a: u8,
    pub f: u8, // flags: Z N H C (bits 7-4), lower nibble always 0
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Registers {
    pub fn new(cgb_mode: bool) -> Self {
        // After boot ROM execution
        Self {
            a: if cgb_mode { 0x11 } else { 0x01 },
            f: 0xB0,
            b: 0x00,
            c: 0x13,
            d: 0x00,
            e: 0xD8,
            h: 0x01,
            l: 0x4D,
            sp: 0xFFFE,
            pc: 0x0100,
        }
    }

    // ─── 16-bit register pairs ────────────────────────────────────────────

    pub fn af(&self) -> u16 { ((self.a as u16) << 8) | (self.f as u16) }
    pub fn bc(&self) -> u16 { ((self.b as u16) << 8) | (self.c as u16) }
    pub fn de(&self) -> u16 { ((self.d as u16) << 8) | (self.e as u16) }
    pub fn hl(&self) -> u16 { ((self.h as u16) << 8) | (self.l as u16) }

    pub fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = (v as u8) & 0xF0; // lower nibble always 0
    }
    pub fn set_bc(&mut self, v: u16) { self.b = (v >> 8) as u8; self.c = v as u8; }
    pub fn set_de(&mut self, v: u16) { self.d = (v >> 8) as u8; self.e = v as u8; }
    pub fn set_hl(&mut self, v: u16) { self.h = (v >> 8) as u8; self.l = v as u8; }

    // ─── Flags ────────────────────────────────────────────────────────────

    pub fn flag_z(&self) -> bool { self.f & 0x80 != 0 }
    pub fn flag_n(&self) -> bool { self.f & 0x40 != 0 }
    pub fn flag_h(&self) -> bool { self.f & 0x20 != 0 }
    pub fn flag_c(&self) -> bool { self.f & 0x10 != 0 }

    pub fn set_flags(&mut self, z: bool, n: bool, h: bool, c: bool) {
        self.f = ((z as u8) << 7) | ((n as u8) << 6) | ((h as u8) << 5) | ((c as u8) << 4);
    }

    pub fn set_z(&mut self, v: bool) { if v { self.f |= 0x80; } else { self.f &= !0x80; } }
    pub fn set_n(&mut self, v: bool) { if v { self.f |= 0x40; } else { self.f &= !0x40; } }
    pub fn set_h(&mut self, v: bool) { if v { self.f |= 0x20; } else { self.f &= !0x20; } }
    pub fn set_c(&mut self, v: bool) { if v { self.f |= 0x10; } else { self.f &= !0x10; } }
}
