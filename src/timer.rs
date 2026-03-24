/// Game Boy hardware timer.
///
/// Registers:
///   0xFF03 – DIV  (16-bit internal counter upper byte, write resets to 0)
///   0xFF05 – TIMA (timer counter, increments at TAC-selected rate)
///   0xFF06 – TMA  (timer modulo, loaded into TIMA on overflow)
///   0xFF07 – TAC  (timer control: bit 2 = enable, bits 1-0 = clock select)

#[derive(Clone)]
pub struct Timer {
    /// Full 16-bit internal divider counter (DIV register is upper 8 bits)
    pub internal_div: u16,
    /// TIMA - timer counter
    pub tima: u8,
    /// TMA - timer modulo
    pub tma: u8,
    /// TAC - timer control
    pub tac: u8,
    /// Set when TIMA overflows
    pub interrupt_requested: bool,
    /// Delay for TIMA overflow (one cycle before interrupt fires)
    overflow_delay: u8,
}

impl Timer {
    pub fn new() -> Self {
        Self {
            internal_div: 0xABCC, // value after boot ROM
            tima: 0,
            tma: 0,
            tac: 0,
            interrupt_requested: false,
            overflow_delay: 0,
        }
    }

    /// Advance the timer by `cycles` T-states (machine cycles × 4).
    pub fn step(&mut self, t_cycles: u32) {
        for _ in 0..t_cycles {
            self.tick();
        }
    }

    fn tick(&mut self) {
        // Handle overflow delay (TIMA is loaded with TMA one cycle after overflow)
        if self.overflow_delay > 0 {
            self.overflow_delay -= 1;
            if self.overflow_delay == 0 {
                self.tima = self.tma;
                self.interrupt_requested = true;
            }
        }

        let old_div = self.internal_div;
        self.internal_div = self.internal_div.wrapping_add(1);

        // Check if the bit selected by TAC fell from 1→0
        if self.tac & 0x04 != 0 {
            let bit_pos = Self::tac_bit(self.tac);
            let old_bit = (old_div >> bit_pos) & 1;
            let new_bit = (self.internal_div >> bit_pos) & 1;
            if old_bit == 1 && new_bit == 0 {
                // Increment TIMA
                let (new_tima, overflow) = self.tima.overflowing_add(1);
                self.tima = new_tima;
                if overflow {
                    self.tima = 0;
                    self.overflow_delay = 4; // fire interrupt 4 T-cycles later
                }
            }
        }
    }

    /// Return the bit of internal_div that controls TIMA increment.
    fn tac_bit(tac: u8) -> u8 {
        match tac & 0x03 {
            0 => 9,  // 4096 Hz   (CPU / 1024)
            1 => 3,  // 262144 Hz (CPU / 16)
            2 => 5,  // 65536 Hz  (CPU / 64)
            3 => 7,  // 16384 Hz  (CPU / 256)
            _ => unreachable!(),
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF03 => (self.internal_div >> 8) as u8,
            0xFF05 => self.tima,
            0xFF06 => self.tma,
            0xFF07 => self.tac | 0xF8, // unused bits read as 1
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF03 => {
                // Writing any value to DIV resets the full counter
                // Also check if timer bit falls due to reset
                let bit_pos = Self::tac_bit(self.tac);
                let old_bit = (self.internal_div >> bit_pos) & 1;
                self.internal_div = 0;
                if self.tac & 0x04 != 0 && old_bit == 1 {
                    // Falling edge – increment TIMA
                    let (new_tima, overflow) = self.tima.overflowing_add(1);
                    self.tima = new_tima;
                    if overflow {
                        self.tima = 0;
                        self.overflow_delay = 4;
                    }
                }
            }
            0xFF05 => {
                // Writing to TIMA during the overflow delay cancels the interrupt
                if self.overflow_delay > 0 {
                    self.overflow_delay = 0;
                }
                self.tima = value;
            }
            0xFF06 => self.tma = value,
            0xFF07 => {
                // Check for falling edge when enabling/changing frequency
                let old_enable = self.tac & 0x04 != 0;
                let new_enable = value & 0x04 != 0;
                let old_bit_pos = Self::tac_bit(self.tac);
                let new_bit_pos = Self::tac_bit(value);
                let old_bit = (self.internal_div >> old_bit_pos) & 1;
                let new_bit = (self.internal_div >> new_bit_pos) & 1;

                let falling_edge = (old_enable && old_bit == 1 && !new_enable)
                    || (old_enable && old_bit == 1 && new_bit == 0);

                self.tac = value & 0x07;
                if falling_edge {
                    let (new_tima, overflow) = self.tima.overflowing_add(1);
                    self.tima = new_tima;
                    if overflow {
                        self.tima = 0;
                        self.overflow_delay = 4;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}
