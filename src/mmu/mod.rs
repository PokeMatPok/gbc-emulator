/// Memory Management Unit
///
/// Routes all memory accesses to the appropriate component.
/// Handles OAM DMA, HDMA, and all I/O registers.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::joypad::Joypad;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

pub struct Mmu {
    pub cart: Box<dyn Cartridge>,
    pub ppu: Ppu,
    pub apu: Apu,
    pub timer: Timer,
    pub joypad: Joypad,
    pub serial: Serial,

    pub wram: [[u8; 0x1000]; 8], // 8 banks × 4KB (GBC WRAM)
    pub wram_bank: usize,         // 0xFF70
    pub hram: [u8; 127],         // 0xFF80–0xFFFE

    pub ie: u8,   // 0xFFFF – Interrupt Enable
    pub iflags: u8, // 0xFF0F – Interrupt Flags

    pub cgb_mode: bool,
    pub double_speed: bool,     // KEY1 bit 7
    pub prepare_speed: bool,    // KEY1 bit 0

    // OAM DMA
    #[allow(dead_code)]
    dma_cycles: u32,
}

impl Mmu {
    pub fn new(cart: Box<dyn Cartridge>, cgb_mode: bool) -> Self {
        Self {
            cart,
            ppu: Ppu::new(cgb_mode),
            apu: Apu::new(),
            timer: Timer::new(),
            joypad: Joypad::new(),
            serial: Serial::new(),
            wram: [[0u8; 0x1000]; 8],
            wram_bank: 1,
            hram: [0u8; 127],
            ie: 0,
            iflags: 0xE1, // unused bits always 1
            cgb_mode,
            double_speed: false,
            prepare_speed: false,
            dma_cycles: 0,
        }
    }

    pub fn read_byte(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => self.ppu.read(addr),
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF => {
                let bank = if self.cgb_mode { self.wram_bank.max(1) } else { 1 };
                self.wram[bank][(addr - 0xD000) as usize]
            }
            0xE000..=0xEFFF => self.wram[0][(addr - 0xE000) as usize], // echo
            0xF000..=0xFDFF => {
                let bank = if self.cgb_mode { self.wram_bank.max(1) } else { 1 };
                self.wram[bank][(addr - 0xF000) as usize]
            }
            0xFE00..=0xFE9F => self.ppu.read(addr),
            0xFEA0..=0xFEFF => 0x00, // prohibited area
            0xFF00 => self.joypad.read(),
            0xFF01..=0xFF02 => self.serial.read(addr),
            0xFF03 | 0xFF05..=0xFF07 => self.timer.read(addr),
            0xFF04 => self.timer.read(0xFF03), // DIV
            0xFF08..=0xFF0E => 0xFF,           // unused
            0xFF0F => self.iflags | 0xE0,
            0xFF10..=0xFF3F => self.apu.read(addr),
            0xFF40..=0xFF4B => self.ppu.read(addr),
            0xFF4C..=0xFF4E => 0xFF, // unused CGB regs
            0xFF4F => self.ppu.read(addr),
            0xFF50 => 0xFF, // boot ROM disable
            0xFF51..=0xFF55 => self.ppu.read(addr),
            0xFF56..=0xFF67 => 0xFF,
            0xFF68..=0xFF6B => self.ppu.read(addr),
            0xFF6C..=0xFF6F => 0xFF,
            0xFF70 => (self.wram_bank as u8) | 0xF8,
            0xFF71..=0xFF7F => 0xFF,
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize],
            0xFFFF => self.ie,
        }
    }

    pub fn write_byte(&mut self, addr: u16, value: u8) {
        match addr {
            0x0000..=0x7FFF => self.cart.write_rom(addr, value),
            0x8000..=0x9FFF => self.ppu.write(addr, value),
            0xA000..=0xBFFF => self.cart.write_ram(addr, value),
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize] = value,
            0xD000..=0xDFFF => {
                let bank = if self.cgb_mode { self.wram_bank.max(1) } else { 1 };
                self.wram[bank][(addr - 0xD000) as usize] = value;
            }
            0xE000..=0xEFFF => self.wram[0][(addr - 0xE000) as usize] = value,
            0xF000..=0xFDFF => {
                let bank = if self.cgb_mode { self.wram_bank.max(1) } else { 1 };
                self.wram[bank][(addr - 0xF000) as usize] = value;
            }
            0xFE00..=0xFE9F => self.ppu.write(addr, value),
            0xFEA0..=0xFEFF => {} // prohibited
            0xFF00 => self.joypad.write(value),
            0xFF01..=0xFF02 => self.serial.write(addr, value),
            0xFF03 | 0xFF05..=0xFF07 => self.timer.write(addr, value),
            0xFF04 => self.timer.write(0xFF03, value), // DIV
            0xFF08..=0xFF0E => {},                     // unused
            0xFF0F => self.iflags = value | 0xE0,
            0xFF10..=0xFF3F => self.apu.write(addr, value),
            0xFF40..=0xFF4B => self.ppu.write(addr, value),
            0xFF4C => {}
            0xFF4D => {
                // KEY1: speed switch
                if self.cgb_mode {
                    self.prepare_speed = value & 0x01 != 0;
                }
            }
            0xFF4E => {}
            0xFF4F => self.ppu.write(addr, value),
            0xFF50 => {} // boot ROM disable (already skipped)
            0xFF51..=0xFF55 => {
                self.ppu.write(addr, value);
                // If HDMA triggered, handle immediately for GDMA
                if addr == 0xFF55 && value & 0x80 == 0 {
                    self.do_gdma();
                }
            }
            0xFF56..=0xFF67 => {}
            0xFF68..=0xFF6B => self.ppu.write(addr, value),
            0xFF6C..=0xFF6F => {}
            0xFF70 => {
                if self.cgb_mode {
                    let bank = (value & 0x07) as usize;
                    self.wram_bank = if bank == 0 { 1 } else { bank };
                }
            }
            0xFF71..=0xFF7F => {}
            0xFF80..=0xFFFE => self.hram[(addr - 0xFF80) as usize] = value,
            0xFFFF => self.ie = value,
        }
    }

    /// Called after each CPU instruction to advance all components.
    pub fn tick(&mut self, t_cycles: u32) {
        // Timer
        self.timer.step(t_cycles);

        // Collect timer interrupt
        if self.timer.interrupt_requested {
            self.timer.interrupt_requested = false;
            self.iflags |= 0x04;
        }

        // PPU
        self.ppu.step(t_cycles);

        // PPU interrupts
        if self.ppu.vblank_irq {
            self.ppu.vblank_irq = false;
            self.iflags |= 0x01;
        }
        if self.ppu.stat_irq {
            self.ppu.stat_irq = false;
            self.iflags |= 0x02;
        }

        // APU
        self.apu.step(t_cycles);

        // OAM DMA
        if self.ppu.dma_active {
            self.step_oam_dma(t_cycles);
        }

        // Serial interrupt
        if self.serial.interrupt_requested {
            self.serial.interrupt_requested = false;
            self.iflags |= 0x08;
        }

        // Joypad interrupt
        if self.joypad.interrupt_requested {
            self.joypad.interrupt_requested = false;
            self.iflags |= 0x10;
        }

        // Cartridge RTC tick
        self.cart.tick(t_cycles);
    }

    fn step_oam_dma(&mut self, t_cycles: u32) {
        if !self.ppu.dma_active { return; }
        // Transfer 1 byte per 4 T-cycles
        for _ in 0..(t_cycles / 4) {
            if self.ppu.dma_offset >= 160 {
                self.ppu.dma_active = false;
                break;
            }
            let src = self.ppu.dma_source | (self.ppu.dma_offset as u16);
            let val = self.dma_read(src);
            self.ppu.oam[self.ppu.dma_offset as usize] = val;
            self.ppu.dma_offset += 1;
        }
    }

    /// Read during OAM DMA (can access ROM/WRAM but not VRAM/OAM)
    fn dma_read(&self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x7FFF => self.cart.read_rom(addr),
            0x8000..=0x9FFF => 0xFF, // restricted
            0xA000..=0xBFFF => self.cart.read_ram(addr),
            0xC000..=0xCFFF => self.wram[0][(addr - 0xC000) as usize],
            0xD000..=0xDFFF => {
                let bank = self.wram_bank.max(1);
                self.wram[bank][(addr - 0xD000) as usize]
            }
            _ => 0xFF,
        }
    }

    /// Execute General Purpose DMA (GDMA) immediately.
    fn do_gdma(&mut self) {
        let src = ((self.ppu.hdma1 as u16) << 8) | ((self.ppu.hdma2 & 0xF0) as u16);
        let dst = 0x8000 | (((self.ppu.hdma3 & 0x1F) as u16) << 8) | ((self.ppu.hdma4 & 0xF0) as u16);
        let len = ((self.ppu.hdma5 & 0x7F) as u16 + 1) * 16;

        for i in 0..len {
            let val = self.read_byte(src.wrapping_add(i));
            self.ppu.write(dst.wrapping_add(i), val);
        }

        self.ppu.hdma5 = 0xFF; // complete
        self.ppu.hdma_active = false;
    }

    /// Trigger speed switch (STOP + KEY1 prepared)
    pub fn switch_speed(&mut self) {
        if self.prepare_speed {
            self.double_speed = !self.double_speed;
            self.prepare_speed = false;
        }
    }

    pub fn is_frame_ready(&self) -> bool {
        self.ppu.frame_ready
    }

    pub fn framebuffer(&self) -> &[u8] {
        &self.ppu.framebuffer
    }
}
