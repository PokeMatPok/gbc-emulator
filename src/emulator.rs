/// Main Emulator struct – ties CPU, MMU, and all components together.

use crate::cpu::Cpu;
use crate::mmu::Mmu;
use crate::joypad::Button;
use crate::cartridge;

pub const CYCLES_PER_FRAME: u32 = 70224; // at 4.194304 MHz, ~59.73 Hz

pub struct Emulator {
    pub cpu: Cpu,
    pub mmu: Mmu,
    cycle_count: u64,
}

impl Emulator {
    /// Create a new emulator with the given ROM bytes.
    pub fn new(rom: Vec<u8>) -> Self {
        let cgb_mode = Self::detect_cgb(&rom);
        let cart = cartridge::load(rom);
        let mmu = Mmu::new(cart, cgb_mode);
        let cpu = Cpu::new(cgb_mode);
        Self { cpu, mmu, cycle_count: 0 }
    }

    fn detect_cgb(rom: &[u8]) -> bool {
        // 0x0143: 0x80 = CGB compatible, 0xC0 = CGB only
        let flag = rom.get(0x0143).copied().unwrap_or(0);
        flag == 0x80 || flag == 0xC0
    }

    /// Run until the PPU completes one full frame (VBlank).
    /// Returns when frame_ready is true.
    pub fn run_frame(&mut self) {
        let mut cycles_this_frame: u32 = 0;
        while cycles_this_frame < CYCLES_PER_FRAME {
            let cycles = self.cpu.step(&mut self.mmu);
            cycles_this_frame += cycles;
            self.cycle_count += cycles as u64;

            // Handle STOP + speed switch
            if self.cpu.stopped {
                self.cpu.stopped = false;
                self.mmu.switch_speed();
            }

            if self.mmu.is_frame_ready() {
                break;
            }
        }
    }

    pub fn press_button(&mut self, button: Button) {
        self.mmu.joypad.press(button);
    }

    pub fn release_button(&mut self, button: Button) {
        self.mmu.joypad.release(button);
    }

    pub fn framebuffer(&self) -> &[u8] {
        self.mmu.framebuffer()
    }

    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.mmu.apu.drain_samples()
    }

    pub fn cart_title(&self) -> &str {
        self.mmu.cart.title()
    }

    pub fn cgb_mode(&self) -> bool {
        self.mmu.cgb_mode
    }

    pub fn cycle_count(&self) -> u64 {
        self.cycle_count
    }
}
