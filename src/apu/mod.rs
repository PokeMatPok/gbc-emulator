/// Audio Processing Unit (APU)
///
/// Generates PCM audio at a configurable sample rate.
/// Four channels: pulse 1 (sweep), pulse 2, wave, noise.

pub const SAMPLE_RATE: u32 = 48000;
const FRAME_SEQUENCER_RATE: u32 = 512; // Hz – clocked every 8192 T-cycles
#[allow(dead_code)]
const T_CYCLES_PER_SECOND: u32 = 4_194_304;

// ─── Duty table ─────────────────────────────────────────────────────────────

const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 0, 0, 0, 0, 0, 0, 1], // 12.5%
    [1, 0, 0, 0, 0, 0, 0, 1], // 25%
    [1, 0, 0, 0, 0, 1, 1, 1], // 50%
    [0, 1, 1, 1, 1, 1, 1, 0], // 75%
];

// ─── Length counter ──────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct LengthCounter {
    counter: u32,
    enabled: bool,
}

impl LengthCounter {
    fn clock(&mut self) -> bool {
        if self.enabled && self.counter > 0 {
            self.counter -= 1;
        }
        self.counter == 0
    }
    fn load(&mut self, max: u32, value: u32) {
        self.counter = if value == 0 { max } else { max - value };
    }
}

// ─── Volume envelope ─────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct VolumeEnvelope {
    volume: u8,
    initial: u8,
    increase: bool,
    period: u8,
    timer: u8,
}

impl VolumeEnvelope {
    fn clock(&mut self) {
        if self.period == 0 { return; }
        if self.timer > 0 { self.timer -= 1; }
        if self.timer == 0 {
            self.timer = self.period;
            if self.increase && self.volume < 15 {
                self.volume += 1;
            } else if !self.increase && self.volume > 0 {
                self.volume -= 1;
            }
        }
    }
    fn trigger(&mut self) {
        self.volume = self.initial;
        self.timer = if self.period == 0 { 8 } else { self.period };
    }
}

// ─── Sweep ───────────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
struct Sweep {
    freq: u16,
    period: u8,
    negate: bool,
    shift: u8,
    timer: u8,
    enabled: bool,
    shadow: u16,
}

impl Sweep {
    fn trigger(&mut self, freq: u16) {
        self.shadow = freq;
        self.timer = if self.period == 0 { 8 } else { self.period };
        self.enabled = self.period != 0 || self.shift != 0;
    }

    fn calculate(&self) -> Option<u16> {
        let delta = self.shadow >> self.shift;
        let new_freq = if self.negate {
            self.shadow.wrapping_sub(delta)
        } else {
            self.shadow + delta
        };
        if new_freq > 2047 { None } else { Some(new_freq) }
    }

    fn clock(&mut self) -> Option<u16> {
        if self.timer > 0 { self.timer -= 1; }
        if self.timer == 0 {
            self.timer = if self.period == 0 { 8 } else { self.period };
            if self.enabled && self.period != 0 {
                return match self.calculate() {
                    Some(f) => {
                        self.shadow = f;
                        self.freq = f;
                        Some(f)
                    }
                    None => None,
                };
            }
        }
        Some(self.freq)
    }
}

// ─── Channel 1 (Pulse + Sweep) ───────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct Channel1 {
    enabled: bool,
    dac_on: bool,
    freq: u16,
    freq_timer: u32,
    duty: u8,
    duty_pos: usize,
    length: LengthCounter,
    envelope: VolumeEnvelope,
    sweep: Sweep,
    // Registers
    nr10: u8,
    nr11: u8,
    nr12: u8,
    nr13: u8,
    nr14: u8,
}

impl Channel1 {
    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_on { return 0.0; }
        let amp = if DUTY_TABLE[self.duty as usize][self.duty_pos] == 1 {
            self.envelope.volume as f32 / 15.0
        } else {
            0.0
        };
        amp
    }

    fn step(&mut self, t_cycles: u32) {
        if !self.enabled { return; }
        let period = (2048 - self.freq as u32) * 4;
        if period == 0 { return; }
        // Accumulate cycles
        let mut remaining = t_cycles;
        while remaining > 0 {
            let to_next = if self.freq_timer == 0 { period } else { self.freq_timer };
            let tick = remaining.min(to_next);
            remaining -= tick;
            if self.freq_timer >= tick {
                self.freq_timer -= tick;
            } else {
                self.freq_timer = period - (tick - self.freq_timer);
            }
            if self.freq_timer == 0 {
                self.duty_pos = (self.duty_pos + 1) % 8;
                self.freq_timer = period;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_on;
        if self.length.counter == 0 {
            self.length.counter = 64;
        }
        self.freq_timer = (2048 - self.freq as u32) * 4;
        self.envelope.trigger();
        self.sweep.trigger(self.freq);
        // If sweep shift != 0, check for immediate overflow
        if self.sweep.shift != 0 {
            if self.sweep.calculate().is_none() {
                self.enabled = false;
            }
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF10 => {
                self.nr10 = value;
                self.sweep.period = (value >> 4) & 0x07;
                self.sweep.negate = value & 0x08 != 0;
                self.sweep.shift = value & 0x07;
            }
            0xFF11 => {
                self.nr11 = value;
                self.duty = (value >> 6) & 0x03;
                self.length.load(64, (value & 0x3F) as u32);
            }
            0xFF12 => {
                self.nr12 = value;
                self.envelope.initial = (value >> 4) & 0x0F;
                self.envelope.increase = value & 0x08 != 0;
                self.envelope.period = value & 0x07;
                self.dac_on = value & 0xF8 != 0;
                if !self.dac_on { self.enabled = false; }
            }
            0xFF13 => {
                self.nr13 = value;
                self.freq = (self.freq & 0x700) | (value as u16);
            }
            0xFF14 => {
                self.nr14 = value;
                self.freq = (self.freq & 0xFF) | (((value as u16) & 0x07) << 8);
                self.length.enabled = value & 0x40 != 0;
                if value & 0x80 != 0 { self.trigger(); }
            }
            _ => {}
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10 => self.nr10 | 0x80,
            0xFF11 => self.nr11 | 0x3F,
            0xFF12 => self.nr12,
            0xFF13 => 0xFF,
            0xFF14 => self.nr14 | 0xBF,
            _ => 0xFF,
        }
    }
}

// ─── Channel 2 (Pulse) ───────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct Channel2 {
    enabled: bool,
    dac_on: bool,
    freq: u16,
    freq_timer: u32,
    duty: u8,
    duty_pos: usize,
    length: LengthCounter,
    envelope: VolumeEnvelope,
    nr21: u8,
    nr22: u8,
    nr23: u8,
    nr24: u8,
}

impl Channel2 {
    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_on { return 0.0; }
        if DUTY_TABLE[self.duty as usize][self.duty_pos] == 1 {
            self.envelope.volume as f32 / 15.0
        } else {
            0.0
        }
    }

    fn step(&mut self, t_cycles: u32) {
        if !self.enabled { return; }
        let period = (2048 - self.freq as u32) * 4;
        if period == 0 { return; }
        let mut remaining = t_cycles;
        while remaining > 0 {
            let to_next = if self.freq_timer == 0 { period } else { self.freq_timer };
            let tick = remaining.min(to_next);
            remaining -= tick;
            if self.freq_timer >= tick {
                self.freq_timer -= tick;
            } else {
                self.freq_timer = period;
            }
            if self.freq_timer == 0 {
                self.duty_pos = (self.duty_pos + 1) % 8;
                self.freq_timer = period;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_on;
        if self.length.counter == 0 { self.length.counter = 64; }
        self.freq_timer = (2048 - self.freq as u32) * 4;
        self.envelope.trigger();
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF16 => {
                self.nr21 = value;
                self.duty = (value >> 6) & 0x03;
                self.length.load(64, (value & 0x3F) as u32);
            }
            0xFF17 => {
                self.nr22 = value;
                self.envelope.initial = (value >> 4) & 0x0F;
                self.envelope.increase = value & 0x08 != 0;
                self.envelope.period = value & 0x07;
                self.dac_on = value & 0xF8 != 0;
                if !self.dac_on { self.enabled = false; }
            }
            0xFF18 => {
                self.nr23 = value;
                self.freq = (self.freq & 0x700) | (value as u16);
            }
            0xFF19 => {
                self.nr24 = value;
                self.freq = (self.freq & 0xFF) | (((value as u16) & 0x07) << 8);
                self.length.enabled = value & 0x40 != 0;
                if value & 0x80 != 0 { self.trigger(); }
            }
            _ => {}
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF16 => self.nr21 | 0x3F,
            0xFF17 => self.nr22,
            0xFF18 => 0xFF,
            0xFF19 => self.nr24 | 0xBF,
            _ => 0xFF,
        }
    }
}

// ─── Channel 3 (Wave) ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Channel3 {
    enabled: bool,
    dac_on: bool,
    freq: u16,
    freq_timer: u32,
    wave_pos: usize,
    output_level: u8, // 0=mute, 1=100%, 2=50%, 3=25%
    length: LengthCounter,
    wave_ram: [u8; 16],
    sample_buf: u8,
    nr30: u8,
    nr31: u8,
    nr32: u8,
    nr33: u8,
    nr34: u8,
}

impl Default for Channel3 {
    fn default() -> Self {
        Self {
            enabled: false,
            dac_on: false,
            freq: 0,
            freq_timer: 0,
            wave_pos: 0,
            output_level: 0,
            length: LengthCounter::default(),
            wave_ram: [0u8; 16],
            sample_buf: 0,
            nr30: 0,
            nr31: 0,
            nr32: 0,
            nr33: 0,
            nr34: 0,
        }
    }
}

impl Channel3 {
    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_on { return 0.0; }
        let raw = self.sample_buf;
        let shifted = match self.output_level {
            0 => 0,
            1 => raw,
            2 => raw >> 1,
            3 => raw >> 2,
            _ => 0,
        };
        (shifted as f32) / 15.0
    }

    fn step(&mut self, t_cycles: u32) {
        if !self.enabled { return; }
        let period = (2048 - self.freq as u32) * 2;
        if period == 0 { return; }
        let mut remaining = t_cycles;
        while remaining > 0 {
            let to_next = if self.freq_timer == 0 { period } else { self.freq_timer };
            let tick = remaining.min(to_next);
            remaining -= tick;
            if self.freq_timer >= tick {
                self.freq_timer -= tick;
            } else {
                self.freq_timer = period;
            }
            if self.freq_timer == 0 {
                self.wave_pos = (self.wave_pos + 1) % 32;
                let byte = self.wave_ram[self.wave_pos / 2];
                self.sample_buf = if self.wave_pos % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                self.freq_timer = period;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_on;
        if self.length.counter == 0 { self.length.counter = 256; }
        self.freq_timer = (2048 - self.freq as u32) * 2;
        self.wave_pos = 0;
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF1A => {
                self.nr30 = value;
                self.dac_on = value & 0x80 != 0;
                if !self.dac_on { self.enabled = false; }
            }
            0xFF1B => {
                self.nr31 = value;
                self.length.load(256, value as u32);
            }
            0xFF1C => {
                self.nr32 = value;
                self.output_level = (value >> 5) & 0x03;
            }
            0xFF1D => {
                self.nr33 = value;
                self.freq = (self.freq & 0x700) | (value as u16);
            }
            0xFF1E => {
                self.nr34 = value;
                self.freq = (self.freq & 0xFF) | (((value as u16) & 0x07) << 8);
                self.length.enabled = value & 0x40 != 0;
                if value & 0x80 != 0 { self.trigger(); }
            }
            0xFF30..=0xFF3F => {
                self.wave_ram[(addr - 0xFF30) as usize] = value;
            }
            _ => {}
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF1A => self.nr30 | 0x7F,
            0xFF1B => 0xFF,
            0xFF1C => self.nr32 | 0x9F,
            0xFF1D => 0xFF,
            0xFF1E => self.nr34 | 0xBF,
            0xFF30..=0xFF3F => self.wave_ram[(addr - 0xFF30) as usize],
            _ => 0xFF,
        }
    }
}

// ─── Channel 4 (Noise) ───────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub struct Channel4 {
    enabled: bool,
    dac_on: bool,
    lfsr: u16,
    freq_timer: u32,
    width_7: bool, // 7-bit LFSR if true
    clock_shift: u8,
    divisor_code: u8,
    length: LengthCounter,
    envelope: VolumeEnvelope,
    nr41: u8,
    nr42: u8,
    nr43: u8,
    nr44: u8,
}

impl Channel4 {
    fn divisor(code: u8) -> u32 {
        match code {
            0 => 8,
            1 => 16,
            2 => 32,
            3 => 48,
            4 => 64,
            5 => 80,
            6 => 96,
            7 => 112,
            _ => 8,
        }
    }

    fn period(&self) -> u32 {
        Self::divisor(self.divisor_code) << self.clock_shift as u32
    }

    fn sample(&self) -> f32 {
        if !self.enabled || !self.dac_on { return 0.0; }
        // LFSR bit 0: 0 = output on
        if self.lfsr & 1 == 0 {
            self.envelope.volume as f32 / 15.0
        } else {
            0.0
        }
    }

    fn step(&mut self, t_cycles: u32) {
        if !self.enabled { return; }
        let period = self.period();
        if period == 0 { return; }
        let mut remaining = t_cycles;
        while remaining > 0 {
            let to_next = if self.freq_timer == 0 { period } else { self.freq_timer };
            let tick = remaining.min(to_next);
            remaining -= tick;
            if self.freq_timer >= tick {
                self.freq_timer -= tick;
            } else {
                self.freq_timer = period;
            }
            if self.freq_timer == 0 {
                let xor = (self.lfsr & 1) ^ ((self.lfsr >> 1) & 1);
                self.lfsr >>= 1;
                self.lfsr |= xor << 14;
                if self.width_7 {
                    self.lfsr = (self.lfsr & !0x40) | (xor << 6);
                }
                self.freq_timer = period;
            }
        }
    }

    fn trigger(&mut self) {
        self.enabled = self.dac_on;
        if self.length.counter == 0 { self.length.counter = 64; }
        self.freq_timer = self.period();
        self.envelope.trigger();
        self.lfsr = 0x7FFF;
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0xFF20 => {
                self.nr41 = value;
                self.length.load(64, (value & 0x3F) as u32);
            }
            0xFF21 => {
                self.nr42 = value;
                self.envelope.initial = (value >> 4) & 0x0F;
                self.envelope.increase = value & 0x08 != 0;
                self.envelope.period = value & 0x07;
                self.dac_on = value & 0xF8 != 0;
                if !self.dac_on { self.enabled = false; }
            }
            0xFF22 => {
                self.nr43 = value;
                self.clock_shift = (value >> 4) & 0x0F;
                self.width_7 = value & 0x08 != 0;
                self.divisor_code = value & 0x07;
            }
            0xFF23 => {
                self.nr44 = value;
                self.length.enabled = value & 0x40 != 0;
                if value & 0x80 != 0 { self.trigger(); }
            }
            _ => {}
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF20 => 0xFF,
            0xFF21 => self.nr42,
            0xFF22 => self.nr43,
            0xFF23 => self.nr44 | 0xBF,
            _ => 0xFF,
        }
    }
}

// ─── APU top level ───────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct Apu {
    pub ch1: Channel1,
    pub ch2: Channel2,
    pub ch3: Channel3,
    pub ch4: Channel4,

    // Master control
    nr50: u8, // 0xFF24 volume + SO output
    nr51: u8, // 0xFF25 channel → output routing
    nr52: u8, // 0xFF26 sound on/off

    // Frame sequencer
    frame_seq_timer: u32,
    frame_seq_step: u8,

    // Sample generation
    sample_timer: u32,
    sample_period: u32,
    pub audio_buffer: Vec<f32>, // interleaved stereo
}

impl Apu {
    pub fn new() -> Self {
        Self {
            ch1: Channel1::default(),
            ch2: Channel2::default(),
            ch3: Channel3::default(),
            ch4: Channel4::default(),
            nr50: 0x77,
            nr51: 0xF3,
            nr52: 0xF1,
            frame_seq_timer: 0,
            frame_seq_step: 0,
            sample_timer: 0,
            sample_period: T_CYCLES_PER_SECOND / SAMPLE_RATE,
            audio_buffer: Vec::with_capacity(4096),
        }
    }

    pub fn step(&mut self, t_cycles: u32) {
        if self.nr52 & 0x80 == 0 {
            return; // APU off
        }

        self.ch1.step(t_cycles);
        self.ch2.step(t_cycles);
        self.ch3.step(t_cycles);
        self.ch4.step(t_cycles);

        // Frame sequencer (512 Hz = one step every 8192 T-cycles)
        self.frame_seq_timer += t_cycles;
        while self.frame_seq_timer >= 8192 {
            self.frame_seq_timer -= 8192;
            self.clock_frame_sequencer();
        }

        // Generate samples
        self.sample_timer += t_cycles;
        while self.sample_timer >= self.sample_period {
            self.sample_timer -= self.sample_period;
            self.generate_sample();
        }
    }

    fn clock_frame_sequencer(&mut self) {
        match self.frame_seq_step {
            0 => { self.clock_length(); }
            1 => {}
            2 => { self.clock_length(); self.clock_sweep(); }
            3 => {}
            4 => { self.clock_length(); }
            5 => {}
            6 => { self.clock_length(); self.clock_sweep(); }
            7 => { self.clock_envelope(); }
            _ => {}
        }
        self.frame_seq_step = (self.frame_seq_step + 1) % 8;
    }

    fn clock_length(&mut self) {
        if self.ch1.length.clock() { self.ch1.enabled = false; }
        if self.ch2.length.clock() { self.ch2.enabled = false; }
        if self.ch3.length.clock() { self.ch3.enabled = false; }
        if self.ch4.length.clock() { self.ch4.enabled = false; }
    }

    fn clock_envelope(&mut self) {
        self.ch1.envelope.clock();
        self.ch2.envelope.clock();
        self.ch4.envelope.clock();
    }

    fn clock_sweep(&mut self) {
        if let Some(f) = self.ch1.sweep.clock() {
            self.ch1.freq = f;
        } else {
            self.ch1.enabled = false;
        }
    }

    fn generate_sample(&mut self) {
        let so1_vol = ((self.nr50 & 0x07) + 1) as f32 / 8.0; // right
        let so2_vol = (((self.nr50 >> 4) & 0x07) + 1) as f32 / 8.0; // left

        let ch1 = self.ch1.sample();
        let ch2 = self.ch2.sample();
        let ch3 = self.ch3.sample();
        let ch4 = self.ch4.sample();

        let mut left = 0.0f32;
        let mut right = 0.0f32;

        // NR51 routing: bits 4-7 = left (SO2), bits 0-3 = right (SO1)
        if self.nr51 & 0x10 != 0 { left += ch1; }
        if self.nr51 & 0x20 != 0 { left += ch2; }
        if self.nr51 & 0x40 != 0 { left += ch3; }
        if self.nr51 & 0x80 != 0 { left += ch4; }
        if self.nr51 & 0x01 != 0 { right += ch1; }
        if self.nr51 & 0x02 != 0 { right += ch2; }
        if self.nr51 & 0x04 != 0 { right += ch3; }
        if self.nr51 & 0x08 != 0 { right += ch4; }

        // Mix and clamp
        let left = (left * so2_vol / 4.0).clamp(-1.0, 1.0);
        let right = (right * so1_vol / 4.0).clamp(-1.0, 1.0);

        self.audio_buffer.push(left);
        self.audio_buffer.push(right);
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF10..=0xFF14 => self.ch1.read(addr),
            0xFF15 => 0xFF,
            0xFF16..=0xFF19 => self.ch2.read(addr),
            0xFF1A..=0xFF1E => self.ch3.read(addr),
            0xFF1F => 0xFF,
            0xFF20..=0xFF23 => self.ch4.read(addr),
            0xFF24 => self.nr50,
            0xFF25 => self.nr51,
            0xFF26 => {
                let ch1 = if self.ch1.enabled { 0x01 } else { 0 };
                let ch2 = if self.ch2.enabled { 0x02 } else { 0 };
                let ch3 = if self.ch3.enabled { 0x04 } else { 0 };
                let ch4 = if self.ch4.enabled { 0x08 } else { 0 };
                (self.nr52 & 0x80) | 0x70 | ch4 | ch3 | ch2 | ch1
            }
            0xFF30..=0xFF3F => self.ch3.read(addr),
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        // Writes to most registers are ignored when APU is off (except NR52 and length counters)
        let apu_on = self.nr52 & 0x80 != 0;
        match addr {
            0xFF10..=0xFF14 => { if apu_on { self.ch1.write(addr, value); } }
            0xFF15 => {}
            0xFF16 => {
                // Length can be written when off
                self.ch2.length.load(64, (value & 0x3F) as u32);
                if apu_on { self.ch2.nr21 = value; self.ch2.duty = (value >> 6) & 0x03; }
            }
            0xFF17..=0xFF19 => { if apu_on { self.ch2.write(addr, value); } }
            0xFF1A..=0xFF1E => { if apu_on { self.ch3.write(addr, value); } }
            0xFF1F => {}
            0xFF20 => {
                self.ch4.length.load(64, (value & 0x3F) as u32);
            }
            0xFF21..=0xFF23 => { if apu_on { self.ch4.write(addr, value); } }
            0xFF24 => self.nr50 = value,
            0xFF25 => self.nr51 = value,
            0xFF26 => {
                let was_on = self.nr52 & 0x80 != 0;
                self.nr52 = value & 0x80;
                if was_on && !apu_on {
                    // Power off: reset all registers
                    self.ch1 = Channel1::default();
                    self.ch2 = Channel2::default();
                    self.ch3 = Channel3::default();
                    self.ch4 = Channel4::default();
                    self.nr50 = 0;
                    self.nr51 = 0;
                }
            }
            0xFF30..=0xFF3F => self.ch3.write(addr, value),
            _ => {}
        }
    }

    /// Drain the audio buffer and return it.
    pub fn drain_samples(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.audio_buffer)
    }
}

impl Default for Apu {
    fn default() -> Self {
        Self::new()
    }
}
