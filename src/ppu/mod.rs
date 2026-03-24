/// Picture Processing Unit (PPU)
///
/// Produces a 160×144 RGBA framebuffer at ~59.7 Hz.
/// Supports both DMG (4 shades of grey) and CGB (full color) modes.

pub const SCREEN_W: usize = 160;
pub const SCREEN_H: usize = 144;
pub const FRAMEBUFFER_SIZE: usize = SCREEN_W * SCREEN_H * 4;

// PPU modes
const MODE_HBLANK: u8 = 0;
const MODE_VBLANK: u8 = 1;
const MODE_OAM_SCAN: u8 = 2;
const MODE_DRAWING: u8 = 3;

// Dots (T-states) per period
const DOTS_OAM_SCAN: u32 = 80;
const DOTS_DRAWING: u32 = 172; // minimum; can be longer
const DOTS_HBLANK: u32 = 204;  // minimum
const DOTS_PER_LINE: u32 = 456;
const LINES_VBLANK: u32 = 10;
#[allow(dead_code)]
const TOTAL_LINES: u32 = 154;

/// One sprite entry from OAM
#[derive(Default, Clone, Copy, Debug)]
struct Sprite {
    y: i32,    // screen y + 16
    x: i32,    // screen x + 8
    tile: u8,
    flags: u8, // bit7=BG over OBJ, bit6=Y flip, bit5=X flip, bit4=palette(DMG), bit3=VRAM bank(CGB), bit2-0=CGB palette
}

impl Sprite {
    fn from_oam(bytes: &[u8]) -> Self {
        Self {
            y: bytes[0] as i32,
            x: bytes[1] as i32,
            tile: bytes[2],
            flags: bytes[3],
        }
    }
    fn priority(&self) -> bool { self.flags & 0x80 != 0 }
    fn y_flip(&self) -> bool { self.flags & 0x40 != 0 }
    fn x_flip(&self) -> bool { self.flags & 0x20 != 0 }
    fn dmg_palette(&self) -> u8 { (self.flags >> 4) & 1 }
    fn cgb_vram_bank(&self) -> u8 { (self.flags >> 3) & 1 }
    fn cgb_palette(&self) -> u8 { self.flags & 0x07 }
}

/// 15-bit BGR555 → RGBA8 color conversion
fn bgr555_to_rgba(lo: u8, hi: u8) -> [u8; 4] {
    let color = (hi as u16) << 8 | lo as u16;
    let r = ((color & 0x001F) as u8) << 3;
    let g = (((color >> 5) & 0x001F) as u8) << 3;
    let b = (((color >> 10) & 0x001F) as u8) << 3;
    [r | r >> 5, g | g >> 5, b | b >> 5, 0xFF]
}

/// DMG greyscale palette
const DMG_PALETTE: [[u8; 4]; 4] = [
    [0xFF, 0xFF, 0xFF, 0xFF], // white
    [0xAA, 0xAA, 0xAA, 0xFF], // light grey
    [0x55, 0x55, 0x55, 0xFF], // dark grey
    [0x00, 0x00, 0x00, 0xFF], // black
];

#[derive(Clone)]
pub struct Ppu {
    // Registers
    pub lcdc: u8,  // 0xFF40
    pub stat: u8,  // 0xFF41
    pub scy: u8,   // 0xFF42
    pub scx: u8,   // 0xFF43
    pub ly: u8,    // 0xFF44
    pub lyc: u8,   // 0xFF45
    pub dma: u8,   // 0xFF46 (trigger OAM DMA)
    pub bgp: u8,   // 0xFF47 (DMG BG palette)
    pub obp0: u8,  // 0xFF48 (DMG OBJ palette 0)
    pub obp1: u8,  // 0xFF49 (DMG OBJ palette 1)
    pub wy: u8,    // 0xFF4A
    pub wx: u8,    // 0xFF4B

    // CGB registers
    pub vbk: u8,            // 0xFF4F (VRAM bank select)
    pub bcps: u8,           // 0xFF68 (BG color palette spec)
    pub bcpd_buf: [u8; 64], // BG color palette RAM
    pub ocps: u8,           // 0xFF6A (OBJ color palette spec)
    pub ocpd_buf: [u8; 64], // OBJ color palette RAM

    // Memory
    pub vram: [[u8; 0x2000]; 2], // 2 banks × 8KB
    pub oam: [u8; 160],

    // Internal state
    mode: u8,
    dots: u32, // dots within current line
    frame_dots: u32,

    pub framebuffer: Vec<u8>, // RGBA8, 160×144
    pub frame_ready: bool,

    // Interrupts
    pub vblank_irq: bool,
    pub stat_irq: bool,

    // OAM DMA
    pub dma_active: bool,
    pub dma_source: u16, // high byte from 0xFF46
    pub dma_offset: u8,

    // CGB mode
    pub cgb_mode: bool,

    // Window internal line counter
    window_line_counter: u8,
    window_triggered: bool,

    // HDMA (CGB)
    pub hdma1: u8, // 0xFF51 source high
    pub hdma2: u8, // 0xFF52 source low
    pub hdma3: u8, // 0xFF53 dest high
    pub hdma4: u8, // 0xFF54 dest low
    pub hdma5: u8, // 0xFF55 length/mode/start
    pub hdma_active: bool,

    // Pixel color data for the current scanline (for BG/Window priority)
    bg_pixel_opaque: [bool; SCREEN_W],
    bg_pixel_priority: [bool; SCREEN_W], // BG priority flag (CGB attr bit 7)
}

impl Ppu {
    pub fn new(cgb_mode: bool) -> Self {
        Self {
            lcdc: 0x91,
            stat: 0x81,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            dma: 0xFF,
            bgp: 0xFC,
            obp0: 0xFF,
            obp1: 0xFF,
            wy: 0,
            wx: 0,
            vbk: 0,
            bcps: 0,
            bcpd_buf: [0xFF; 64],
            ocps: 0,
            ocpd_buf: [0xFF; 64],
            vram: [[0u8; 0x2000]; 2],
            oam: [0u8; 160],
            mode: MODE_OAM_SCAN,
            dots: 0,
            frame_dots: 0,
            framebuffer: vec![0xFFu8; FRAMEBUFFER_SIZE],
            frame_ready: false,
            vblank_irq: false,
            stat_irq: false,
            dma_active: false,
            dma_source: 0,
            dma_offset: 0,
            cgb_mode,
            window_line_counter: 0,
            window_triggered: false,
            hdma1: 0xFF,
            hdma2: 0xFF,
            hdma3: 0xFF,
            hdma4: 0xFF,
            hdma5: 0xFF,
            hdma_active: false,
            bg_pixel_opaque: [false; SCREEN_W],
            bg_pixel_priority: [false; SCREEN_W],
        }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0x9FFF => {
                let bank = if self.cgb_mode { self.vbk as usize & 1 } else { 0 };
                // VRAM inaccessible during pixel transfer in DMG mode
                if !self.cgb_mode && self.mode == MODE_DRAWING {
                    return 0xFF;
                }
                self.vram[bank][(addr - 0x8000) as usize]
            }
            0xFE00..=0xFE9F => {
                if self.mode >= MODE_OAM_SCAN { return 0xFF; }
                self.oam[(addr - 0xFE00) as usize]
            }
            0xFF40 => self.lcdc,
            0xFF41 => (self.stat & 0xF8) | (if self.ly == self.lyc { 0x04 } else { 0 }) | (self.mode & 0x03),
            0xFF42 => self.scy,
            0xFF43 => self.scx,
            0xFF44 => self.ly,
            0xFF45 => self.lyc,
            0xFF46 => self.dma,
            0xFF47 => self.bgp,
            0xFF48 => self.obp0,
            0xFF49 => self.obp1,
            0xFF4A => self.wy,
            0xFF4B => self.wx,
            0xFF4F => self.vbk | 0xFE,
            0xFF51 => self.hdma1,
            0xFF52 => self.hdma2,
            0xFF53 => self.hdma3,
            0xFF54 => self.hdma4,
            0xFF55 => self.hdma5,
            0xFF68 => self.bcps,
            0xFF69 => {
                let idx = (self.bcps & 0x3F) as usize;
                self.bcpd_buf[idx]
            }
            0xFF6A => self.ocps,
            0xFF6B => {
                let idx = (self.ocps & 0x3F) as usize;
                self.ocpd_buf[idx]
            }
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                let bank = if self.cgb_mode { self.vbk as usize & 1 } else { 0 };
                if !self.cgb_mode && self.mode == MODE_DRAWING { return; }
                self.vram[bank][(addr - 0x8000) as usize] = value;
            }
            0xFE00..=0xFE9F => {
                if self.mode >= MODE_OAM_SCAN { return; }
                self.oam[(addr - 0xFE00) as usize] = value;
            }
            0xFF40 => {
                let lcd_was_on = self.lcdc & 0x80 != 0;
                self.lcdc = value;
                if lcd_was_on && value & 0x80 == 0 {
                    // LCD turned off
                    self.ly = 0;
                    self.mode = MODE_HBLANK;
                    self.dots = 0;
                    self.frame_dots = 0;
                    // Clear framebuffer to white
                    for b in self.framebuffer.iter_mut() { *b = 0xFF; }
                }
            }
            0xFF41 => self.stat = (self.stat & 0x07) | (value & 0xF8),
            0xFF42 => self.scy = value,
            0xFF43 => self.scx = value,
            0xFF44 => {} // LY is read-only
            0xFF45 => self.lyc = value,
            0xFF46 => {
                self.dma = value;
                self.dma_source = (value as u16) << 8;
                self.dma_active = true;
                self.dma_offset = 0;
            }
            0xFF47 => self.bgp = value,
            0xFF48 => self.obp0 = value,
            0xFF49 => self.obp1 = value,
            0xFF4A => self.wy = value,
            0xFF4B => self.wx = value,
            0xFF4F => self.vbk = value & 0x01,
            0xFF51 => self.hdma1 = value,
            0xFF52 => self.hdma2 = value,
            0xFF53 => self.hdma3 = value,
            0xFF54 => self.hdma4 = value,
            0xFF55 => {
                if value & 0x80 == 0 {
                    // GDMA: transfer immediately (handled by MMU)
                    self.hdma5 = value;
                    self.hdma_active = true; // signal to MMU
                } else {
                    // HDMA: transfer 16 bytes per HBlank
                    self.hdma5 = value & 0x7F;
                    self.hdma_active = true;
                }
            }
            0xFF68 => self.bcps = value,
            0xFF69 => {
                let idx = (self.bcps & 0x3F) as usize;
                self.bcpd_buf[idx] = value;
                if self.bcps & 0x80 != 0 {
                    self.bcps = 0x80 | ((self.bcps + 1) & 0x3F);
                }
            }
            0xFF6A => self.ocps = value,
            0xFF6B => {
                let idx = (self.ocps & 0x3F) as usize;
                self.ocpd_buf[idx] = value;
                if self.ocps & 0x80 != 0 {
                    self.ocps = 0x80 | ((self.ocps + 1) & 0x3F);
                }
            }
            _ => {}
        }
    }

    /// Step the PPU by `t_cycles` T-states.
    /// Returns (vblank_irq, stat_irq).
    pub fn step(&mut self, t_cycles: u32) {
        if self.lcdc & 0x80 == 0 {
            return; // LCD off
        }
        self.frame_ready = false;

        for _ in 0..t_cycles {
            self.dots += 1;
            self.frame_dots += 1;

            // OAM DMA: 160 bytes, 1 byte per 4 T-states (runs independently)
            // (handled in MMU – just track here conceptually)

            match self.mode {
                MODE_OAM_SCAN => {
                    if self.dots >= DOTS_OAM_SCAN {
                        self.dots = 0;
                        self.mode = MODE_DRAWING;
                    }
                }
                MODE_DRAWING => {
                    if self.dots >= DOTS_DRAWING {
                        self.dots = 0;
                        self.render_scanline();
                        self.mode = MODE_HBLANK;
                        // HDMA HBlank trigger
                        self.check_stat_irq();
                    }
                }
                MODE_HBLANK => {
                    if self.dots >= DOTS_HBLANK {
                        self.dots = 0;
                        self.ly += 1;

                        if self.ly >= SCREEN_H as u8 {
                            self.mode = MODE_VBLANK;
                            self.vblank_irq = true;
                            self.frame_ready = true;
                            self.window_line_counter = 0;
                            self.window_triggered = false;
                        } else {
                            self.mode = MODE_OAM_SCAN;
                        }
                        self.check_lyc_irq();
                        self.check_stat_irq();
                    }
                }
                MODE_VBLANK => {
                    if self.dots >= DOTS_PER_LINE {
                        self.dots = 0;
                        self.ly += 1;
                        self.check_lyc_irq();
                        if self.ly >= TOTAL_LINES as u8 {
                            self.ly = 0;
                            self.mode = MODE_OAM_SCAN;
                            self.frame_dots = 0;
                            self.check_stat_irq();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn check_lyc_irq(&mut self) {
        if self.ly == self.lyc && self.stat & 0x40 != 0 {
            self.stat_irq = true;
        }
    }

    fn check_stat_irq(&mut self) {
        let trigger = match self.mode {
            MODE_HBLANK => self.stat & 0x08 != 0,
            MODE_VBLANK => self.stat & 0x10 != 0,
            MODE_OAM_SCAN => self.stat & 0x20 != 0,
            _ => false,
        };
        if trigger { self.stat_irq = true; }
    }

    // ─── Scanline renderer ─────────────────────────────────────────────────

    fn render_scanline(&mut self) {
        let ly = self.ly as usize;
        if ly >= SCREEN_H { return; }

        let row_base = ly * SCREEN_W;

        // Reset priority arrays
        self.bg_pixel_opaque = [false; SCREEN_W];
        self.bg_pixel_priority = [false; SCREEN_W];

        // BG + Window
        if self.lcdc & 0x01 != 0 || self.cgb_mode {
            self.render_bg_window(ly, row_base);
        } else if !self.cgb_mode {
            // BG disabled on DMG: fill white
            for x in 0..SCREEN_W {
                let fb = (row_base + x) * 4;
                self.framebuffer[fb..fb + 4].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
            }
        }

        // Sprites
        if self.lcdc & 0x02 != 0 {
            self.render_sprites(ly, row_base);
        }
    }

    fn render_bg_window(&mut self, ly: usize, row_base: usize) {
        let lcdc = self.lcdc;
        let scx = self.scx as usize;
        let scy = self.scy as usize;
        let wx = self.wx.wrapping_sub(7) as usize;
        let wy = self.wy as usize;

        // Which tile map base address?
        let bg_map_base: usize = if lcdc & 0x08 != 0 { 0x1C00 } else { 0x1800 };
        let win_map_base: usize = if lcdc & 0x40 != 0 { 0x1C00 } else { 0x1800 };
        // Which tile data area?
        let tile_data_mode = lcdc & 0x10 != 0; // true = 0x8000 (unsigned), false = 0x8800 (signed)

        // Check if window is visible this line
        let window_enabled = lcdc & 0x20 != 0 && ly >= wy;

        if window_enabled && !self.window_triggered {
            self.window_triggered = true;
        }

        for x in 0..SCREEN_W {
            let (use_window, tx, ty) = if window_enabled && x >= wx {
                let wlc = self.window_line_counter as usize;
                let wx_off = x - wx;
                (true, wx_off, wlc)
            } else {
                let bx = (x + scx) & 0xFF;
                let by = (ly + scy) & 0xFF;
                (false, bx, by)
            };

            let map_base = if use_window { win_map_base } else { bg_map_base };
            let tile_col = tx / 8;
            let tile_row = ty / 8;
            let tile_x = tx % 8;
            let tile_y = ty % 8;
            let map_addr = map_base + tile_row * 32 + tile_col;

            // Tile index (bank 0)
            let tile_idx = self.vram[0][map_addr];
            // CGB tile attributes (bank 1)
            let tile_attr = if self.cgb_mode { self.vram[1][map_addr] } else { 0 };

            let tile_data_bank = if self.cgb_mode && tile_attr & 0x08 != 0 { 1usize } else { 0 };
            let flip_x = self.cgb_mode && tile_attr & 0x20 != 0;
            let flip_y = self.cgb_mode && tile_attr & 0x40 != 0;
            let bg_priority = self.cgb_mode && tile_attr & 0x80 != 0;
            let cgb_pal_idx = (tile_attr & 0x07) as usize;

            // Tile data address
            let tile_addr = if tile_data_mode {
                (tile_idx as usize) * 16
            } else {
                // Signed addressing: 0x1000 + (i8 offset) * 16
                let signed = tile_idx as i8 as i32;
                (0x1000 + signed * 16) as usize
            };

            let eff_tile_y = if flip_y { 7 - tile_y } else { tile_y };
            let row_lo = self.vram[tile_data_bank][tile_addr + eff_tile_y * 2];
            let row_hi = self.vram[tile_data_bank][tile_addr + eff_tile_y * 2 + 1];

            let eff_tile_x = if flip_x { tile_x } else { 7 - tile_x };
            let color_id = ((row_lo >> eff_tile_x) & 1) | (((row_hi >> eff_tile_x) & 1) << 1);

            self.bg_pixel_opaque[x] = color_id != 0;
            self.bg_pixel_priority[x] = bg_priority;

            let rgba = if self.cgb_mode {
                let pal_idx = cgb_pal_idx * 8 + color_id as usize * 2;
                let lo = self.bcpd_buf[pal_idx];
                let hi = self.bcpd_buf[pal_idx + 1];
                bgr555_to_rgba(lo, hi)
            } else {
                let color_2bit = (self.bgp >> (color_id * 2)) & 0x03;
                DMG_PALETTE[color_2bit as usize]
            };

            let fb = (row_base + x) * 4;
            self.framebuffer[fb..fb + 4].copy_from_slice(&rgba);
        }

        if window_enabled && self.window_triggered {
            self.window_line_counter = self.window_line_counter.wrapping_add(1);
        }
    }

    fn render_sprites(&mut self, ly: usize, row_base: usize) {
        let sprite_height: i32 = if self.lcdc & 0x04 != 0 { 16 } else { 8 };
        let mut sprites: Vec<Sprite> = Vec::with_capacity(10);

        // Collect up to 10 sprites visible on this scanline
        for i in 0..40 {
            if sprites.len() >= 10 { break; }
            let base = i * 4;
            let s = Sprite::from_oam(&self.oam[base..base + 4]);
            let screen_y = s.y - 16;
            let ly_i = ly as i32;
            if ly_i >= screen_y && ly_i < screen_y + sprite_height {
                sprites.push(s);
            }
        }

        // DMG: sprites are drawn in reverse order (lower OAM index = higher priority)
        // CGB: sprites are drawn in OAM order
        if !self.cgb_mode {
            sprites.sort_by(|a, b| a.x.cmp(&b.x));
        }

        for sprite in sprites.iter().rev() {
            let screen_y = sprite.y - 16;
            let tile_y = (ly as i32 - screen_y) as usize;
            let tile_y = if sprite.y_flip() {
                (sprite_height as usize - 1) - tile_y
            } else {
                tile_y
            };

            // In 8×16 mode, bit 0 of tile index is ignored
            let tile_idx = if sprite_height == 16 {
                (sprite.tile & 0xFE) as usize + if tile_y >= 8 { 1 } else { 0 }
            } else {
                sprite.tile as usize
            };
            let tile_y = tile_y % 8;

            let bank = if self.cgb_mode { sprite.cgb_vram_bank() as usize } else { 0 };
            let tile_addr = tile_idx * 16;
            let row_lo = self.vram[bank][tile_addr + tile_y * 2];
            let row_hi = self.vram[bank][tile_addr + tile_y * 2 + 1];

            for tile_x in 0..8usize {
                let screen_x = sprite.x - 8 + tile_x as i32;
                if screen_x < 0 || screen_x >= SCREEN_W as i32 { continue; }
                let screen_x = screen_x as usize;

                let bit = if sprite.x_flip() { tile_x } else { 7 - tile_x };
                let color_id = ((row_lo >> bit) & 1) | (((row_hi >> bit) & 1) << 1);
                if color_id == 0 { continue; } // transparent

                // Priority: sprite hidden behind BG if BG-over-OBJ or BG color != 0
                let bg_wins = (sprite.priority() || self.bg_pixel_priority[screen_x])
                    && self.bg_pixel_opaque[screen_x];

                if bg_wins && self.lcdc & 0x01 != 0 { continue; }

                let rgba = if self.cgb_mode {
                    let cgb_pal = sprite.cgb_palette() as usize;
                    let pal_idx = cgb_pal * 8 + color_id as usize * 2;
                    let lo = self.ocpd_buf[pal_idx];
                    let hi = self.ocpd_buf[pal_idx + 1];
                    bgr555_to_rgba(lo, hi)
                } else {
                    let palette = if sprite.dmg_palette() == 0 { self.obp0 } else { self.obp1 };
                    let color_2bit = (palette >> (color_id * 2)) & 0x03;
                    DMG_PALETTE[color_2bit as usize]
                };

                let fb = (row_base + screen_x) * 4;
                self.framebuffer[fb..fb + 4].copy_from_slice(&rgba);
            }
        }
    }
}
