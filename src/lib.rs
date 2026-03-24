pub mod apu;
pub mod cartridge;
pub mod cpu;
pub mod emulator;
pub mod joypad;
pub mod mmu;
pub mod ppu;
pub mod serial;
pub mod timer;

use wasm_bindgen::prelude::*;
use emulator::Emulator;
use joypad::Button;
use ppu::{SCREEN_W, SCREEN_H};

#[wasm_bindgen]
pub struct GbcEmulator {
    inner: Emulator,
}

#[wasm_bindgen]
impl GbcEmulator {
    /// Create a new emulator from ROM bytes.
    #[wasm_bindgen(constructor)]
    pub fn new(rom: &[u8]) -> GbcEmulator {
        #[cfg(feature = "panic_hook")]
        console_error_panic_hook::set_once();

        GbcEmulator {
            inner: Emulator::new(rom.to_vec()),
        }
    }

    /// Run the emulator until one frame is complete.
    #[wasm_bindgen]
    pub fn run_frame(&mut self) {
        self.inner.run_frame();
    }

    /// Get the RGBA framebuffer (160×144×4 bytes).
    #[wasm_bindgen]
    pub fn framebuffer(&self) -> Vec<u8> {
        self.inner.framebuffer().to_vec()
    }

    /// Get the framebuffer as a Uint8ClampedArray (zero-copy via pointer).
    #[wasm_bindgen]
    pub fn framebuffer_ptr(&self) -> *const u8 {
        self.inner.framebuffer().as_ptr()
    }

    /// Width of the framebuffer in pixels.
    #[wasm_bindgen]
    pub fn width(&self) -> u32 {
        SCREEN_W as u32
    }

    /// Height of the framebuffer in pixels.
    #[wasm_bindgen]
    pub fn height(&self) -> u32 {
        SCREEN_H as u32
    }

    /// Drain audio samples (interleaved stereo f32).
    #[wasm_bindgen]
    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.inner.drain_audio()
    }

    /// Get the cartridge title.
    #[wasm_bindgen]
    pub fn cart_title(&self) -> String {
        self.inner.cart_title().to_string()
    }

    /// Whether the ROM runs in CGB (Game Boy Color) mode.
    #[wasm_bindgen]
    pub fn cgb_mode(&self) -> bool {
        self.inner.cgb_mode()
    }

    // ─── Button inputs ────────────────────────────────────────────────────

    #[wasm_bindgen]
    pub fn press_a(&mut self)       { self.inner.press_button(Button::A); }
    #[wasm_bindgen]
    pub fn press_b(&mut self)       { self.inner.press_button(Button::B); }
    #[wasm_bindgen]
    pub fn press_start(&mut self)   { self.inner.press_button(Button::Start); }
    #[wasm_bindgen]
    pub fn press_select(&mut self)  { self.inner.press_button(Button::Select); }
    #[wasm_bindgen]
    pub fn press_up(&mut self)      { self.inner.press_button(Button::Up); }
    #[wasm_bindgen]
    pub fn press_down(&mut self)    { self.inner.press_button(Button::Down); }
    #[wasm_bindgen]
    pub fn press_left(&mut self)    { self.inner.press_button(Button::Left); }
    #[wasm_bindgen]
    pub fn press_right(&mut self)   { self.inner.press_button(Button::Right); }

    #[wasm_bindgen]
    pub fn release_a(&mut self)     { self.inner.release_button(Button::A); }
    #[wasm_bindgen]
    pub fn release_b(&mut self)     { self.inner.release_button(Button::B); }
    #[wasm_bindgen]
    pub fn release_start(&mut self) { self.inner.release_button(Button::Start); }
    #[wasm_bindgen]
    pub fn release_select(&mut self){ self.inner.release_button(Button::Select); }
    #[wasm_bindgen]
    pub fn release_up(&mut self)    { self.inner.release_button(Button::Up); }
    #[wasm_bindgen]
    pub fn release_down(&mut self)  { self.inner.release_button(Button::Down); }
    #[wasm_bindgen]
    pub fn release_left(&mut self)  { self.inner.release_button(Button::Left); }
    #[wasm_bindgen]
    pub fn release_right(&mut self) { self.inner.release_button(Button::Right); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minimal_rom_load() {
        // A minimal ROM with valid header enough to construct emulator
        let mut rom = vec![0u8; 0x150];
        rom[0x0100] = 0x00; // NOP
        rom[0x0101] = 0x18; // JR
        rom[0x0102] = 0xFE; // -2 (infinite loop)
        rom[0x0104] = 0xCE; // Nintendo logo start (fake)
        rom[0x0147] = 0x00; // ROM Only
        rom[0x0148] = 0x00; // 32KB ROM
        rom[0x0149] = 0x00; // No RAM
        let emu = Emulator::new(rom);
        assert!(!emu.cgb_mode());
        assert_eq!(emu.framebuffer().len(), 160 * 144 * 4);
    }

    #[test]
    fn test_cgb_detection() {
        let mut rom = vec![0u8; 0x150];
        rom[0x0143] = 0xC0; // CGB only
        let emu = Emulator::new(rom);
        assert!(emu.cgb_mode());
    }

    #[test]
    fn test_frame_runs() {
        let mut rom = vec![0u8; 0x8000];
        rom[0x0100] = 0x00; // NOP loop
        rom[0x0101] = 0x18;
        rom[0x0102] = 0xFE;
        let mut emu = Emulator::new(rom);
        // Should not panic
        emu.run_frame();
        assert_eq!(emu.framebuffer().len(), 160 * 144 * 4);
    }

    #[test]
    fn test_joypad_press_release() {
        let mut rom = vec![0u8; 0x150];
        let mut emu = Emulator::new(rom);
        emu.press_button(Button::A);
        emu.release_button(Button::A);
    }
}
