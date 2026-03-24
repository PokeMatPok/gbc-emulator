/// Game Boy joypad input handler.
///
/// Register 0xFF00 (P1/JOYP):
///   Bit 5 – Select Action buttons  (0 = select)
///   Bit 4 – Select Direction buttons (0 = select)
///   Bit 3 – Down  / Start  (0 = pressed)
///   Bit 2 – Up    / Select (0 = pressed)
///   Bit 1 – Left  / B      (0 = pressed)
///   Bit 0 – Right / A      (0 = pressed)

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

#[derive(Default, Clone)]
pub struct Joypad {
    /// Action buttons: A, B, Select, Start (active-low bitmask, bit 0..3)
    action: u8,
    /// Direction buttons: Right, Left, Up, Down (active-low bitmask, bit 0..3)
    direction: u8,
    /// The value written to P1 (selects which group to read)
    select: u8,
    /// Set when a button is pressed (for interrupt generation)
    pub interrupt_requested: bool,
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            action: 0x0F,    // all released (bits high)
            direction: 0x0F, // all released
            select: 0xFF,
            interrupt_requested: false,
        }
    }

    /// Read P1 register (0xFF00)
    pub fn read(&self) -> u8 {
        let mut result = self.select | 0xC0; // upper two bits unused
        if self.select & 0x20 == 0 {
            // action buttons selected
            result = (result & 0xF0) | (self.action & 0x0F);
        }
        if self.select & 0x10 == 0 {
            // direction buttons selected
            result = (result & 0xF0) | (self.direction & 0x0F);
        }
        result
    }

    /// Write to P1 register (only bits 4-5 are writable)
    pub fn write(&mut self, value: u8) {
        self.select = value & 0x30;
    }

    pub fn press(&mut self, button: Button) {
        let prev = self.read();
        match button {
            Button::A => self.action &= !0x01,
            Button::B => self.action &= !0x02,
            Button::Select => self.action &= !0x04,
            Button::Start => self.action &= !0x08,
            Button::Right => self.direction &= !0x01,
            Button::Left => self.direction &= !0x02,
            Button::Up => self.direction &= !0x04,
            Button::Down => self.direction &= !0x08,
        }
        // Joypad interrupt on high-to-low transition of any input line
        let now = self.read();
        if prev & 0x0F != 0 && now & 0x0F != prev & 0x0F {
            self.interrupt_requested = true;
        }
    }

    pub fn release(&mut self, button: Button) {
        match button {
            Button::A => self.action |= 0x01,
            Button::B => self.action |= 0x02,
            Button::Select => self.action |= 0x04,
            Button::Start => self.action |= 0x08,
            Button::Right => self.direction |= 0x01,
            Button::Left => self.direction |= 0x02,
            Button::Up => self.direction |= 0x04,
            Button::Down => self.direction |= 0x08,
        }
    }
}
