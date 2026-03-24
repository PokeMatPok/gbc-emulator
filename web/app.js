/**
 * GBC Emulator – Web Frontend
 *
 * Uses WebGPU for rendering when available, falls back to Canvas 2D.
 * Audio is handled via Web Audio API.
 */

// ─── Constants ───────────────────────────────────────────────────────────────

const SCREEN_W = 160;
const SCREEN_H = 144;
const TARGET_FPS = 60;
const FRAME_INTERVAL_MS = 1000 / TARGET_FPS;

// Keyboard → button mapping
const KEY_MAP = {
  ArrowUp:    'up',
  ArrowDown:  'down',
  ArrowLeft:  'left',
  ArrowRight: 'right',
  KeyZ:       'a',
  KeyX:       'b',
  Enter:      'start',
  ShiftRight: 'select',
  ShiftLeft:  'select',
  Space:      'select',
};

// Gamepad button → button mapping (standard layout)
const GAMEPAD_MAP = {
  0: 'a',
  1: 'b',
  8: 'select',
  9: 'start',
  12: 'up',
  13: 'down',
  14: 'left',
  15: 'right',
};

// ─── App state ───────────────────────────────────────────────────────────────

let emulator = null;          // GbcEmulator WASM instance
let wasmModule = null;        // The imported WASM module
let renderer = null;          // WebGpuRenderer | Canvas2dRenderer
let audioCtx = null;
let audioNode = null;
let audioQueue = [];
let muted = false;

let running = false;
let rafHandle = null;
let lastTimestamp = 0;
let frameCount = 0;
let fpsAccum = 0;

const gamepadState = {};      // track pressed gamepad buttons

// ─── DOM refs ────────────────────────────────────────────────────────────────

const canvas     = document.getElementById('screen');
const overlay    = document.getElementById('screen-overlay');
const cartTitle  = document.getElementById('cart-title');
const fpsCounter = document.getElementById('fps-counter');
const modeBadge  = document.getElementById('mode-badge');
const romInput   = document.getElementById('rom-input');
const dropZone   = document.getElementById('drop-zone');
const muteBtn    = document.getElementById('mute-btn');
const fsBtn      = document.getElementById('fullscreen-btn');

// ─── Initialise WASM ─────────────────────────────────────────────────────────

async function initWasm() {
  try {
    // Try loading from /pkg/ (wasm-pack output) or fallback path
    const paths = ['./pkg/gbc_emulator.js', './gbc_emulator.js'];
    let mod = null;
    for (const p of paths) {
      try {
        mod = await import(p);
        break;
      } catch { /* try next */ }
    }
    if (!mod) throw new Error('WASM module not found – run `npm run build` first');
    await mod.default(); // initialise WASM
    wasmModule = mod;
    return true;
  } catch (err) {
    console.error('WASM init failed:', err);
    overlay.querySelector('p').textContent = '⚠️ ' + err.message;
    return false;
  }
}

// ─── Renderer ────────────────────────────────────────────────────────────────

/** WebGPU renderer – uploads the 160×144 RGBA texture each frame */
class WebGpuRenderer {
  constructor(device, context, format) {
    this.device = device;
    this.context = context;

    // Create texture for the GB screen
    this.texture = device.createTexture({
      size: { width: SCREEN_W, height: SCREEN_H },
      format: 'rgba8unorm',
      usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST,
    });

    // Sampler (nearest-neighbour for pixel-perfect scaling)
    this.sampler = device.createSampler({
      magFilter: 'nearest',
      minFilter: 'nearest',
    });

    // Shader: full-screen quad, sample texture
    const shaderSrc = /* wgsl */`
      struct VertexOut {
        @builtin(position) pos: vec4<f32>,
        @location(0) uv: vec2<f32>,
      };

      @vertex
      fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOut {
        // Two triangles covering the screen
        var positions = array<vec2<f32>, 6>(
          vec2(-1.0, -1.0), vec2( 1.0, -1.0), vec2(-1.0,  1.0),
          vec2(-1.0,  1.0), vec2( 1.0, -1.0), vec2( 1.0,  1.0)
        );
        var uvs = array<vec2<f32>, 6>(
          vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(0.0, 0.0),
          vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(1.0, 0.0)
        );
        var out: VertexOut;
        out.pos = vec4(positions[vi], 0.0, 1.0);
        out.uv = uvs[vi];
        return out;
      }

      @group(0) @binding(0) var tex: texture_2d<f32>;
      @group(0) @binding(1) var samp: sampler;

      @fragment
      fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
        return textureSample(tex, samp, in.uv);
      }
    `;

    const module = device.createShaderModule({ code: shaderSrc });

    const bindGroupLayout = device.createBindGroupLayout({
      entries: [
        { binding: 0, visibility: GPUShaderStage.FRAGMENT, texture: { sampleType: 'float' } },
        { binding: 1, visibility: GPUShaderStage.FRAGMENT, sampler: { type: 'filtering' } },
      ],
    });

    this.bindGroup = device.createBindGroup({
      layout: bindGroupLayout,
      entries: [
        { binding: 0, resource: this.texture.createView() },
        { binding: 1, resource: this.sampler },
      ],
    });

    this.pipeline = device.createRenderPipeline({
      layout: device.createPipelineLayout({ bindGroupLayouts: [bindGroupLayout] }),
      vertex: { module, entryPoint: 'vs_main' },
      fragment: {
        module,
        entryPoint: 'fs_main',
        targets: [{ format }],
      },
      primitive: { topology: 'triangle-list' },
    });
  }

  draw(rgbaPixels) {
    // Upload new framebuffer
    this.device.queue.writeTexture(
      { texture: this.texture },
      rgbaPixels,
      { bytesPerRow: SCREEN_W * 4 },
      { width: SCREEN_W, height: SCREEN_H },
    );

    const encoder = this.device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [{
        view: this.context.getCurrentTexture().createView(),
        clearValue: { r: 0, g: 0, b: 0, a: 1 },
        loadOp: 'clear',
        storeOp: 'store',
      }],
    });

    pass.setPipeline(this.pipeline);
    pass.setBindGroup(0, this.bindGroup);
    pass.draw(6);
    pass.end();

    this.device.queue.submit([encoder.finish()]);
  }
}

/** Canvas 2D fallback renderer */
class Canvas2dRenderer {
  constructor(canvas) {
    this.ctx = canvas.getContext('2d');
    this.imageData = this.ctx.createImageData(SCREEN_W, SCREEN_H);
  }

  draw(rgbaPixels) {
    this.imageData.data.set(rgbaPixels);
    this.ctx.putImageData(this.imageData, 0, 0);
  }
}

async function initRenderer() {
  // Try WebGPU first
  if (navigator.gpu) {
    try {
      const adapter = await navigator.gpu.requestAdapter();
      if (adapter) {
        const device = await adapter.requestDevice();
        const context = canvas.getContext('webgpu');
        const format = navigator.gpu.getPreferredCanvasFormat();
        context.configure({ device, format, alphaMode: 'opaque' });
        modeBadge.textContent = 'WebGPU';
        return new WebGpuRenderer(device, context, format);
      }
    } catch (e) {
      console.warn('WebGPU init failed, falling back to Canvas 2D:', e);
    }
  }
  // Canvas 2D fallback
  modeBadge.textContent = 'Canvas 2D';
  return new Canvas2dRenderer(canvas);
}

// ─── Audio ───────────────────────────────────────────────────────────────────

function initAudio() {
  audioCtx = new (window.AudioContext || window.webkitAudioContext)({
    sampleRate: 48000,
    latencyHint: 'interactive',
  });
}

function queueAudio(samples) {
  if (!audioCtx || muted || samples.length === 0) return;
  const frameCount = samples.length / 2;
  const buf = audioCtx.createBuffer(2, frameCount, 48000);
  const left  = buf.getChannelData(0);
  const right = buf.getChannelData(1);
  for (let i = 0; i < frameCount; i++) {
    left[i]  = samples[i * 2];
    right[i] = samples[i * 2 + 1];
  }
  const node = audioCtx.createBufferSource();
  node.buffer = buf;
  node.connect(audioCtx.destination);
  // Schedule audio just ahead of "now" to avoid gaps
  const when = Math.max(audioCtx.currentTime, (audioQueue.at(-1)?.endTime ?? audioCtx.currentTime));
  node.start(when);
  audioQueue.push({ node, endTime: when + buf.duration });
  // Trim old entries
  audioQueue = audioQueue.filter(e => e.endTime > audioCtx.currentTime - 0.5);
}

// ─── ROM loading ─────────────────────────────────────────────────────────────

async function loadRom(buffer) {
  if (!wasmModule) return;

  const romBytes = new Uint8Array(buffer);
  if (running) stopEmulation();

  emulator = new wasmModule.GbcEmulator(romBytes);

  const title = emulator.cart_title() || 'Unknown ROM';
  cartTitle.textContent = title;
  document.title = `GBC – ${title}`;

  overlay.classList.add('hidden');
  startEmulation();
}

// ─── Emulation loop ───────────────────────────────────────────────────────────

function startEmulation() {
  if (running) return;
  running = true;
  lastTimestamp = performance.now();
  frameCount = 0;
  fpsAccum = 0;

  // Resume audio context (browsers require user gesture)
  if (audioCtx?.state === 'suspended') audioCtx.resume();

  rafHandle = requestAnimationFrame(loop);
}

function stopEmulation() {
  running = false;
  if (rafHandle) { cancelAnimationFrame(rafHandle); rafHandle = null; }
}

let accumMs = 0;

function loop(timestamp) {
  if (!running) return;

  const delta = timestamp - lastTimestamp;
  lastTimestamp = timestamp;

  // Run emulator
  if (emulator) {
    accumMs += Math.min(delta, 50); // clamp to avoid spiral of death
    while (accumMs >= FRAME_INTERVAL_MS) {
      accumMs -= FRAME_INTERVAL_MS;
      emulator.run_frame();

      // Render
      const fb = emulator.framebuffer();
      renderer.draw(fb);

      // Audio
      const samples = emulator.drain_audio();
      queueAudio(samples);
    }
  }

  // FPS counter
  fpsAccum += delta;
  frameCount++;
  if (fpsAccum >= 1000) {
    fpsCounter.textContent = `FPS: ${frameCount}`;
    frameCount = 0;
    fpsAccum = 0;
  }

  // Gamepad polling
  pollGamepad();

  rafHandle = requestAnimationFrame(loop);
}

// ─── Input ───────────────────────────────────────────────────────────────────

function pressBtn(btn) {
  if (!emulator) return;
  switch (btn) {
    case 'a':      emulator.press_a();      break;
    case 'b':      emulator.press_b();      break;
    case 'start':  emulator.press_start();  break;
    case 'select': emulator.press_select(); break;
    case 'up':     emulator.press_up();     break;
    case 'down':   emulator.press_down();   break;
    case 'left':   emulator.press_left();   break;
    case 'right':  emulator.press_right();  break;
  }
}

function releaseBtn(btn) {
  if (!emulator) return;
  switch (btn) {
    case 'a':      emulator.release_a();      break;
    case 'b':      emulator.release_b();      break;
    case 'start':  emulator.release_start();  break;
    case 'select': emulator.release_select(); break;
    case 'up':     emulator.release_up();     break;
    case 'down':   emulator.release_down();   break;
    case 'left':   emulator.release_left();   break;
    case 'right':  emulator.release_right();  break;
  }
}

// Keyboard
document.addEventListener('keydown', e => {
  const btn = KEY_MAP[e.code];
  if (btn) { e.preventDefault(); pressBtn(btn); markBtn(btn, true); }
});
document.addEventListener('keyup', e => {
  const btn = KEY_MAP[e.code];
  if (btn) { e.preventDefault(); releaseBtn(btn); markBtn(btn, false); }
});

// On-screen buttons
document.querySelectorAll('[data-btn]').forEach(el => {
  const btn = el.dataset.btn;

  const down = () => { pressBtn(btn); el.classList.add('pressed'); };
  const up   = () => { releaseBtn(btn); el.classList.remove('pressed'); };

  el.addEventListener('pointerdown',  e => { e.preventDefault(); down(); });
  el.addEventListener('pointerup',    e => { e.preventDefault(); up(); });
  el.addEventListener('pointerleave', e => { up(); });
});

function markBtn(btn, pressed) {
  const el = document.querySelector(`[data-btn="${btn}"]`);
  if (el) el.classList.toggle('pressed', pressed);
}

// Gamepad
function pollGamepad() {
  const pads = navigator.getGamepads?.();
  if (!pads) return;
  for (const pad of pads) {
    if (!pad) continue;
    for (const [idx, btn] of Object.entries(GAMEPAD_MAP)) {
      const pressed = pad.buttons[idx]?.pressed ?? false;
      const wasPressed = gamepadState[idx] ?? false;
      if (pressed && !wasPressed) { pressBtn(btn); gamepadState[idx] = true; }
      if (!pressed && wasPressed) { releaseBtn(btn); gamepadState[idx] = false; }
    }
    // Axes → D-pad
    const ax = pad.axes[0] ?? 0;
    const ay = pad.axes[1] ?? 0;
    const threshold = 0.5;
    handleAxis('left',  ax < -threshold, 'left');
    handleAxis('right', ax >  threshold, 'right');
    handleAxis('up',    ay < -threshold, 'up');
    handleAxis('down',  ay >  threshold, 'down');
  }
}

const axisState = {};
function handleAxis(id, isPressed, btn) {
  if (isPressed && !axisState[id]) { pressBtn(btn); axisState[id] = true; }
  if (!isPressed && axisState[id]) { releaseBtn(btn); axisState[id] = false; }
}

// ─── ROM drag & drop ─────────────────────────────────────────────────────────

document.addEventListener('dragover', e => {
  e.preventDefault();
  dropZone.classList.remove('hidden');
});
document.addEventListener('dragleave', e => {
  if (!e.relatedTarget) dropZone.classList.add('hidden');
});
document.addEventListener('drop', e => {
  e.preventDefault();
  dropZone.classList.add('hidden');
  const file = e.dataTransfer.files[0];
  if (file) readFile(file);
});

overlay.addEventListener('click', () => romInput.click());
romInput.addEventListener('change', () => {
  const file = romInput.files[0];
  if (file) readFile(file);
});

function readFile(file) {
  const reader = new FileReader();
  reader.onload = () => loadRom(reader.result);
  reader.readAsArrayBuffer(file);
}

// ─── UI buttons ──────────────────────────────────────────────────────────────

muteBtn.addEventListener('click', () => {
  muted = !muted;
  muteBtn.textContent = muted ? '🔇' : '🔊';
  if (audioCtx) {
    if (muted) audioCtx.suspend();
    else       audioCtx.resume();
  }
});

fsBtn.addEventListener('click', () => {
  const wrapper = document.getElementById('screen-wrapper');
  if (!document.fullscreenElement) wrapper.requestFullscreen();
  else document.exitFullscreen();
});

// ─── Boot ─────────────────────────────────────────────────────────────────────

(async () => {
  renderer = await initRenderer();
  initAudio();

  const ok = await initWasm();
  if (ok) {
    overlay.classList.remove('hidden');
  }
})();
