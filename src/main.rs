use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: gbc-emulator <rom.gb>");
        eprintln!("  Headless mode: runs the ROM for N frames and reports cycle count.");
        eprintln!("  gbc-emulator <rom.gb> [frames=60]");
        std::process::exit(1);
    }

    let rom_path = &args[1];
    let rom = fs::read(rom_path).unwrap_or_else(|e| {
        eprintln!("Failed to read ROM: {e}");
        std::process::exit(1);
    });

    let mut emu = gbc_emulator::emulator::Emulator::new(rom);
    println!("Loaded: '{}' | CGB mode: {}", emu.cart_title(), emu.cgb_mode());

    let frames = args.get(2)
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(60);

    for i in 0..frames {
        emu.run_frame();
        if i % 60 == 59 || i == frames - 1 {
            println!("Frame {:>4}/{} | total cycles: {}", i + 1, frames, emu.cycle_count());
        }
    }

    println!("Done.");
}
