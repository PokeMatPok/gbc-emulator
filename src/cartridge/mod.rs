use std::fmt;

/// Cartridge trait – implemented by each MBC type.
pub trait Cartridge: fmt::Debug + Send {
    fn read_rom(&self, addr: u16) -> u8;
    fn write_rom(&mut self, addr: u16, value: u8);
    fn read_ram(&self, addr: u16) -> u8;
    fn write_ram(&mut self, addr: u16, value: u8);
    fn title(&self) -> &str;
    fn has_battery(&self) -> bool;
    /// For RTC: advance time by `cycles` T-states
    fn tick(&mut self, _t_cycles: u32) {}
}

/// Parse the cartridge ROM and return the correct MBC implementation.
pub fn load(rom: Vec<u8>) -> Box<dyn Cartridge> {
    let mbc_type = rom.get(0x0147).copied().unwrap_or(0);
    let rom_size_code = rom.get(0x0148).copied().unwrap_or(0);
    let ram_size_code = rom.get(0x0149).copied().unwrap_or(0);

    let _num_rom_banks = match rom_size_code {
        0x00 => 2,
        0x01 => 4,
        0x02 => 8,
        0x03 => 16,
        0x04 => 32,
        0x05 => 64,
        0x06 => 128,
        0x07 => 256,
        0x08 => 512,
        0x52 => 72,
        0x53 => 80,
        0x54 => 96,
        _ => 2,
    };

    let ram_size = match ram_size_code {
        0x00 => 0,
        0x01 => 2 * 1024,
        0x02 => 8 * 1024,
        0x03 => 32 * 1024,
        0x04 => 128 * 1024,
        0x05 => 64 * 1024,
        _ => 0,
    };

    let has_battery = matches!(mbc_type, 0x03 | 0x06 | 0x09 | 0x0D | 0x0F | 0x10 | 0x13 | 0x1B | 0x1E | 0x22 | 0xFF);

    match mbc_type {
        0x00 => Box::new(RomOnly::new(rom)),
        0x01 | 0x02 | 0x03 => Box::new(Mbc1::new(rom, ram_size, has_battery)),
        0x05 | 0x06 => Box::new(Mbc2::new(rom, has_battery)),
        0x0F | 0x10 | 0x11 | 0x12 | 0x13 => Box::new(Mbc3::new(rom, ram_size, has_battery)),
        0x19 | 0x1A | 0x1B | 0x1C | 0x1D | 0x1E => Box::new(Mbc5::new(rom, ram_size, has_battery)),
        _ => Box::new(RomOnly::new(rom)),
    }
}

// ─── Helper ─────────────────────────────────────────────────────────────────

fn extract_title(rom: &[u8]) -> String {
    let bytes = &rom[0x0134..=0x0143];
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).to_string()
}

// ─── ROM Only ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct RomOnly {
    rom: Vec<u8>,
    title: String,
}

impl RomOnly {
    fn new(rom: Vec<u8>) -> Self {
        let title = extract_title(&rom);
        Self { rom, title }
    }
}

impl Cartridge for RomOnly {
    fn read_rom(&self, addr: u16) -> u8 {
        self.rom.get(addr as usize).copied().unwrap_or(0xFF)
    }
    fn write_rom(&mut self, _addr: u16, _value: u8) {}
    fn read_ram(&self, _addr: u16) -> u8 { 0xFF }
    fn write_ram(&mut self, _addr: u16, _value: u8) {}
    fn title(&self) -> &str { &self.title }
    fn has_battery(&self) -> bool { false }
}

// ─── MBC1 ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Mbc1 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    rom_bank: usize,
    ram_bank: usize,
    ram_enabled: bool,
    banking_mode: u8, // 0 = ROM banking, 1 = RAM banking
    has_battery: bool,
    title: String,
}

impl Mbc1 {
    fn new(rom: Vec<u8>, ram_size: usize, has_battery: bool) -> Self {
        let title = extract_title(&rom);
        Self {
            rom,
            ram: vec![0u8; ram_size.max(8 * 1024)],
            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            banking_mode: 0,
            has_battery,
            title,
        }
    }

    fn effective_rom0_offset(&self) -> usize {
        if self.banking_mode == 1 {
            ((self.ram_bank & 0x03) << 5) * 0x4000
        } else {
            0
        }
    }
}

impl Cartridge for Mbc1 {
    fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => {
                let base = self.effective_rom0_offset();
                self.rom.get(base + addr as usize).copied().unwrap_or(0xFF)
            }
            0x4000..=0x7FFF => {
                let offset = (addr - 0x4000) as usize;
                let bank = self.rom_bank & ((self.rom.len() / 0x4000).next_power_of_two() - 1);
                let base = bank * 0x4000;
                self.rom.get(base + offset).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => {
                self.ram_enabled = value & 0x0F == 0x0A;
            }
            0x2000..=0x3FFF => {
                let lower5 = (value & 0x1F) as usize;
                let lower5 = if lower5 == 0 { 1 } else { lower5 };
                self.rom_bank = (self.rom_bank & !0x1F) | lower5;
            }
            0x4000..=0x5FFF => {
                let bits = (value & 0x03) as usize;
                if self.banking_mode == 0 {
                    self.rom_bank = (self.rom_bank & 0x1F) | (bits << 5);
                } else {
                    self.ram_bank = bits;
                }
            }
            0x6000..=0x7FFF => {
                self.banking_mode = value & 0x01;
            }
            _ => {}
        }
    }

    fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled { return 0xFF; }
        let bank = if self.banking_mode == 1 { self.ram_bank } else { 0 };
        let offset = bank * 0x2000 + (addr - 0xA000) as usize;
        self.ram.get(offset).copied().unwrap_or(0xFF)
    }

    fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enabled { return; }
        let bank = if self.banking_mode == 1 { self.ram_bank } else { 0 };
        let offset = bank * 0x2000 + (addr - 0xA000) as usize;
        if let Some(b) = self.ram.get_mut(offset) { *b = value; }
    }

    fn title(&self) -> &str { &self.title }
    fn has_battery(&self) -> bool { self.has_battery }
}

// ─── MBC2 ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Mbc2 {
    rom: Vec<u8>,
    ram: [u8; 512],  // 512 × 4-bit cells stored in lower nibble
    rom_bank: usize,
    ram_enabled: bool,
    has_battery: bool,
    title: String,
}

impl Mbc2 {
    fn new(rom: Vec<u8>, has_battery: bool) -> Self {
        let title = extract_title(&rom);
        Self {
            rom,
            ram: [0u8; 512],
            rom_bank: 1,
            ram_enabled: false,
            has_battery,
            title,
        }
    }
}

impl Cartridge for Mbc2 {
    fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF),
            0x4000..=0x7FFF => {
                let offset = (addr - 0x4000) as usize;
                let bank = self.rom_bank & 0x0F;
                self.rom.get(bank * 0x4000 + offset).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    fn write_rom(&mut self, addr: u16, value: u8) {
        if addr <= 0x3FFF {
            if addr & 0x0100 == 0 {
                self.ram_enabled = value & 0x0F == 0x0A;
            } else {
                let bank = (value & 0x0F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
        }
    }

    fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled { return 0xFF; }
        let idx = (addr - 0xA000) as usize & 0x1FF;
        0xF0 | (self.ram[idx] & 0x0F)
    }

    fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enabled { return; }
        let idx = (addr - 0xA000) as usize & 0x1FF;
        self.ram[idx] = value & 0x0F;
    }

    fn title(&self) -> &str { &self.title }
    fn has_battery(&self) -> bool { self.has_battery }
}

// ─── MBC3 ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct Rtc {
    seconds: u8,
    minutes: u8,
    hours: u8,
    days_lo: u8,
    days_hi: u8, // bit0=day MSB, bit6=halt, bit7=carry
    latched: Option<[u8; 5]>,
    sub_seconds: u32, // accumulated T-cycles
}

impl Rtc {
    fn new() -> Self {
        Self { seconds: 0, minutes: 0, hours: 0, days_lo: 0, days_hi: 0, latched: None, sub_seconds: 0 }
    }

    fn tick(&mut self, t_cycles: u32) {
        if self.days_hi & 0x40 != 0 { return; } // halted
        self.sub_seconds += t_cycles;
        // 4194304 T-cycles per second
        while self.sub_seconds >= 4_194_304 {
            self.sub_seconds -= 4_194_304;
            self.seconds = self.seconds.wrapping_add(1);
            if self.seconds >= 60 {
                self.seconds = 0;
                self.minutes = self.minutes.wrapping_add(1);
                if self.minutes >= 60 {
                    self.minutes = 0;
                    self.hours = self.hours.wrapping_add(1);
                    if self.hours >= 24 {
                        self.hours = 0;
                        let days = (self.days_lo as u16) | (((self.days_hi & 0x01) as u16) << 8);
                        let days = days.wrapping_add(1);
                        self.days_lo = days as u8;
                        if days & 0x100 != 0 {
                            self.days_hi |= 0x01;
                        } else {
                            self.days_hi &= !0x01;
                        }
                        if days >= 512 {
                            self.days_hi |= 0x80; // carry
                            self.days_lo = 0;
                            self.days_hi &= !0x01;
                        }
                    }
                }
            }
        }
    }

    fn latch(&mut self) {
        self.latched = Some([self.seconds, self.minutes, self.hours, self.days_lo, self.days_hi]);
    }

    fn read_reg(&self, reg: u8) -> u8 {
        let (s, m, h, dl, dh) = if let Some(l) = &self.latched {
            (l[0], l[1], l[2], l[3], l[4])
        } else {
            (self.seconds, self.minutes, self.hours, self.days_lo, self.days_hi)
        };
        match reg {
            0x08 => s,
            0x09 => m,
            0x0A => h,
            0x0B => dl,
            0x0C => dh,
            _ => 0xFF,
        }
    }

    fn write_reg(&mut self, reg: u8, value: u8) {
        match reg {
            0x08 => self.seconds = value & 0x3F,
            0x09 => self.minutes = value & 0x3F,
            0x0A => self.hours = value & 0x1F,
            0x0B => self.days_lo = value,
            0x0C => self.days_hi = value & 0xC1,
            _ => {}
        }
    }
}

#[derive(Debug)]
pub struct Mbc3 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    rom_bank: usize,
    ram_bank: u8,
    ram_rtc_enabled: bool,
    has_battery: bool,
    rtc: Rtc,
    latch_prev: u8,
    title: String,
}

impl Mbc3 {
    fn new(rom: Vec<u8>, ram_size: usize, has_battery: bool) -> Self {
        let title = extract_title(&rom);
        Self {
            rom,
            ram: vec![0u8; ram_size.max(8 * 1024)],
            rom_bank: 1,
            ram_bank: 0,
            ram_rtc_enabled: false,
            has_battery,
            rtc: Rtc::new(),
            latch_prev: 0xFF,
            title,
        }
    }
}

impl Cartridge for Mbc3 {
    fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF),
            0x4000..=0x7FFF => {
                let offset = (addr - 0x4000) as usize;
                let bank = self.rom_bank & ((self.rom.len() / 0x4000).next_power_of_two() - 1);
                self.rom.get(bank * 0x4000 + offset).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_rtc_enabled = value & 0x0F == 0x0A,
            0x2000..=0x3FFF => {
                let bank = (value & 0x7F) as usize;
                self.rom_bank = if bank == 0 { 1 } else { bank };
            }
            0x4000..=0x5FFF => self.ram_bank = value,
            0x6000..=0x7FFF => {
                if self.latch_prev == 0x00 && value == 0x01 {
                    self.rtc.latch();
                }
                self.latch_prev = value;
            }
            _ => {}
        }
    }

    fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_rtc_enabled { return 0xFF; }
        match self.ram_bank {
            0x00..=0x03 => {
                let offset = self.ram_bank as usize * 0x2000 + (addr - 0xA000) as usize;
                self.ram.get(offset).copied().unwrap_or(0xFF)
            }
            0x08..=0x0C => self.rtc.read_reg(self.ram_bank),
            _ => 0xFF,
        }
    }

    fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_rtc_enabled { return; }
        match self.ram_bank {
            0x00..=0x03 => {
                let offset = self.ram_bank as usize * 0x2000 + (addr - 0xA000) as usize;
                if let Some(b) = self.ram.get_mut(offset) { *b = value; }
            }
            0x08..=0x0C => self.rtc.write_reg(self.ram_bank, value),
            _ => {}
        }
    }

    fn title(&self) -> &str { &self.title }
    fn has_battery(&self) -> bool { self.has_battery }
    fn tick(&mut self, t_cycles: u32) { self.rtc.tick(t_cycles); }
}

// ─── MBC5 ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct Mbc5 {
    rom: Vec<u8>,
    ram: Vec<u8>,
    rom_bank: usize,  // 9-bit bank number
    ram_bank: usize,
    ram_enabled: bool,
    has_battery: bool,
    title: String,
}

impl Mbc5 {
    fn new(rom: Vec<u8>, ram_size: usize, has_battery: bool) -> Self {
        let title = extract_title(&rom);
        Self {
            rom,
            ram: vec![0u8; ram_size.max(8 * 1024)],
            rom_bank: 1,
            ram_bank: 0,
            ram_enabled: false,
            has_battery,
            title,
        }
    }
}

impl Cartridge for Mbc5 {
    fn read_rom(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x3FFF => self.rom.get(addr as usize).copied().unwrap_or(0xFF),
            0x4000..=0x7FFF => {
                let offset = (addr - 0x4000) as usize;
                let num_banks = (self.rom.len() / 0x4000).next_power_of_two();
                let bank = self.rom_bank & (num_banks - 1);
                self.rom.get(bank * 0x4000 + offset).copied().unwrap_or(0xFF)
            }
            _ => 0xFF,
        }
    }

    fn write_rom(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram_enabled = value & 0x0F == 0x0A,
            0x2000..=0x2FFF => {
                // Lower 8 bits of ROM bank number
                self.rom_bank = (self.rom_bank & 0x100) | (value as usize);
            }
            0x3000..=0x3FFF => {
                // Bit 8 of ROM bank number
                self.rom_bank = (self.rom_bank & 0xFF) | (((value & 0x01) as usize) << 8);
            }
            0x4000..=0x5FFF => self.ram_bank = (value & 0x0F) as usize,
            _ => {}
        }
    }

    fn read_ram(&self, addr: u16) -> u8 {
        if !self.ram_enabled { return 0xFF; }
        let offset = self.ram_bank * 0x2000 + (addr - 0xA000) as usize;
        self.ram.get(offset).copied().unwrap_or(0xFF)
    }

    fn write_ram(&mut self, addr: u16, value: u8) {
        if !self.ram_enabled { return; }
        let offset = self.ram_bank * 0x2000 + (addr - 0xA000) as usize;
        if let Some(b) = self.ram.get_mut(offset) { *b = value; }
    }

    fn title(&self) -> &str { &self.title }
    fn has_battery(&self) -> bool { self.has_battery }
}
