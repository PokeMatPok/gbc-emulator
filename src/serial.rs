/// Minimal serial port stub.
/// Real link-cable emulation is not implemented; this just satisfies register reads.

#[derive(Default, Clone)]
pub struct Serial {
    pub sb: u8,   // 0xFF01 – Serial transfer data
    pub sc: u8,   // 0xFF02 – Serial transfer control
    pub interrupt_requested: bool,
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF01 => self.sb,
            0xFF02 => self.sc | 0x7E, // unused bits are 1
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF01 => self.sb = value,
            0xFF02 => {
                self.sc = value;
                // If transfer start bit set and internal clock selected, immediately "finish"
                if value & 0x81 == 0x81 {
                    self.sb = 0xFF; // nothing connected
                    self.sc &= !0x80; // clear transfer start
                    self.interrupt_requested = true;
                }
            }
            _ => {}
        }
    }
}
