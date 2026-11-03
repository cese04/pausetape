// The pixel room beside "Pause. Record. Pause.": sitting by a boombox, someone opens its deck,
// puts in the tape from their lap, closes it and presses play; the reels turn and notes float
// up while they listen; then they take the tape out, turn it over and put it down again.
// Drawn at 160 × 120 and scaled up by whole pixels, so they stay square. Afternoon in a light
// theme, night in a dark one. Under Reduce motion it's one still picture; ?room=7.5 also stops
// it there, at 7.5 s into the loop (for checking a moment).
//
// HOW IT'S MADE (for changing it later)
//
// No image files and no libraries: every frame is drawn from scratch with fillRect on a
// 160 × 120 canvas (see `data-pixel-room` in index.html), then the browser scales the canvas up
// without smoothing (`image-rendering: pixelated` in style.css). `fit()` picks the largest
// whole number of device pixels per picture pixel that fits the column, so pixels stay square.
//
// Coordinates are picture pixels: x from 0 (left) to 159, y from 0 (top) to 119. Always draw
// on whole numbers. The layout, roughly:
//
//   y  0– 8   fairy lights                     x   1– 58  window, curtains, cat on the sill
//   y  0–84   the wall (FLOOR = 85)            x  24– 85  boombox (BX, BW), on the floor
//   y 85–119  the floor, rug, shadows          x  84–132  them, on the cushion (SEAT = 108)
//                                              x 122–159  shelf, books, plant; mug at 133–143
//
// Depth: things on the wall are drawn a little smaller than the person and the boombox, which
// are nearer. As a guide, the person sits about 67 px tall (~90 cm), about 0.75 px per cm near
// the front, a little less on the wall. Keep new things to that scale.
//
// Each frame, `draw(t)` paints back to front, so what comes later covers what came before:
//   room (wall, light, floor, rug, fairy lights, window and cat, shelf) → mug → cushion →
//   the person (hoodie, legs, head) → tape on the lap → far arm → boombox → near arm and the
//   tape in that hand → notes.
// The near arm comes after the boombox because it reaches in front of it.
//
// The pieces, top to bottom in this file:
// - Colours: DAY and NIGHT for the room (switched by the system's light or dark theme), C for
//   what's in it, which looks the same day and night.
// - Drawing helpers: rect, px, oval (an ellipse row by row), ditherOval (every other pixel, or
//   one in four, for light and shading), stroke (a thick line, for arms and the antenna),
//   sprite (a small picture written as rows of "#"), and hash (a repeatable "random" number,
//   so twinkles and the level meter need no state).
// - Where things are: the boombox's keys and door, the person's seat, shoulders, knees and lap.
//   Moving something usually means changing these numbers and the drawing that uses them.
// - The story: PARTS is a list of [seconds, what happens]. `stateAt(t)` plays every finished
//   part to its end and the current one partway (p from 0 to 1), which gives the hand's target,
//   where the tape is ("lap", "hand" or "deck"), how open the door is (0–1), which key is down,
//   whether it's playing, how far into the song (`listen`) and the turn of the tape (`flip`).
//   To change the story, add, remove or retime parts; the loop's length follows. A part that
//   only matters while it lasts (a key held down) checks `done` so it doesn't stay on.
// - The arms: `reach()` is a two-part arm whose upper arm and forearm keep their length
//   (UPPER, FORE); the elbow bends down and out to its side. An arm never stretches: when the
//   hand's target is far, `lean()` bends them at the hips towards it, and `bent()` moves each
//   row of the hoodie, the shoulders and the head by how high they are above the hips.
// - The room, the cat, the shelf, the mug, the person, the tape, the boombox, the notes: one
//   function each, named for what they draw.
// - Running it: frames are drawn at 15 a second (the time is rounded to 1/15 s, for the 8-bit
//   feel), only while the canvas is on screen; one still frame (STILL) under Reduce motion.
//
// To check a change: open site/index.html in a browser (no server needed) and scroll to the
// story, or add ?room=<seconds> to stop at one moment. For the night, switch the system to dark,
// or in Edge/Chrome DevTools: Ctrl+Shift+P › "Emulate CSS prefers-color-scheme: dark".
// Screenshots of a moment without opening anything (Windows; Chrome works the same):
//   msedge --headless=new --window-size=640,480 --screenshot=room.png "file:///…/site/index.html?room=7.5"
// with a page that holds just the canvas and this script, to see the picture alone; add
// --force-dark-mode --blink-settings=preferredColorScheme=0 for the night.

(() => {
  const canvas = document.querySelector("[data-pixel-room]");
  if (!canvas) return;
  const W = 160, H = 120;
  const g = canvas.getContext("2d");
  const still = matchMedia("(prefers-reduced-motion: reduce)");
  const dark = matchMedia("(prefers-color-scheme: dark)");

  // The room's colours, for the afternoon and the night. What's in it keeps its own (C).
  const DAY = {
    wall: "#f3dfc8", stripe: "#ecd3b9", glow: "#f9eada", base: "#c99f7a",
    floor: "#c58d5e", floorLine: "#a8744a", rug: "#dca468", rugEdge: "#b97f4b",
    sky: ["#ffd9a8", "#f9b495", "#f09aa0"], sun: "#fff1b8", cloud: "#fff7ec",
    wood: "#a8744a", curtain: "#b9cfae", curtainShade: "#9cb68f",
    pot: "#d9825b", plant: "#7fb069", plantDark: "#5a8c4e", wire: "#7a6470", bulbOff: "#d8c4b0",
    photo: "#bfe0e6", mug: "#9fd8cb", mugStripe: "#f6f0e2", steam: "#ffffff",
    book: ["#e07a6a", "#6fb3c7", "#f2c46d", "#9b86c9"],
  };
  const NIGHT = {
    wall: "#4b3f52", stripe: "#45394b", glow: "#6e5a62", base: "#2f2733",
    floor: "#5a4038", floorLine: "#46302b", rug: "#7a5a48", rugEdge: "#5e4436",
    sky: ["#1b2140", "#232a52", "#2e3566"], moon: "#f4ecc8", star: "#fff6d8", starDim: "#5d628a",
    wood: "#6e4c3e", curtain: "#56705e", curtainShade: "#465d4e",
    pot: "#a65f45", plant: "#5b8a55", plantDark: "#3f6a40", wire: "#2a2230", bulbOff: "#5a4a58",
    photo: "#6f8fa0", mug: "#6fb3a6", mugStripe: "#d8cfbd", steam: "#d9d0e2",
    book: ["#b85a4e", "#4f8799", "#c49a4f", "#7a66a3"],
  };
  const C = {
    out: "#2b1d22", white: "#fffaf0",
    skin: "#f7d3b8", skinShade: "#e3ae94", blush: "#f1b8a6", mouth: "#b5524b",
    hair: "#4a2f27", hairLight: "#6e4636",
    hoodie: "#8e8b93", hoodieShade: "#76727c", hoodieLight: "#a7a4ab", string: "#e8e4ea",
    pants: "#4d5675", pantsShade: "#3a4260", socks: "#efe7da", cushion: "#e2b65c", cushionShade: "#c4963f",
    body: "#e8765c", bodyLight: "#f3a189", bodyShade: "#bd5843", handle: "#4a3740", metal: "#c7c2cf",
    rim: "#5a4650", grill: "#3a2d34", grillDark: "#291f25", cone: "#7a6570", cap: "#c9bac2",
    panel: "#f4e7d1", lcd: "#26302b", lcdOn: "#8be0a4", lcdOff: "#3b4a42",
    key: "#efe4d2", keyDown: "#cbbca6", red: "#e04f45", mint: "#7fd1b0",
    cavity: "#1d1519", door: "#e8d8bf", doorLight: "#fbf1df", doorShade: "#cdbb9f", doorInner: "#d6c2a3",
    reel: "#f4ead8", tape: "#7a5440", label: "#f6efe2", shell: "#3d3236", shellDark: "#2b2226",
    sides: ["#e8765c", "#6fc7b5"], star: "#ffd36e", heart: "#e07a6a",
    cat: "#e9a15b", catDark: "#c47a3a", catPink: "#f4a6a6",
    bulbs: ["#ffd36e", "#ff9bb3", "#8fe0d0", "#ffb36b"],
  };
  let P = dark.matches ? NIGHT : DAY;

  // Drawing on whole pixels.
  function rect(x, y, w, h, c) {
    g.fillStyle = c;
    g.fillRect(x, y, w, h);
  }
  function px(x, y, c) {
    rect(x, y, 1, 1, c);
  }
  // The half-width of an ellipse's row, so its edge falls on whole pixels.
  const half = (rx, ry, y) => Math.round(rx * Math.sqrt(Math.max(0, 1 - (y / (ry + 0.5)) ** 2)));
  function oval(cx, cy, rx, ry, c) {
    g.fillStyle = c;
    for (let y = -ry; y <= ry; y++) {
      const w = half(rx, ry, y);
      g.fillRect(cx - w, cy + y, 2 * w + 1, 1);
    }
  }
  // Every other pixel of an ellipse (or one in four, `sparse`): 8-bit light and shading.
  function ditherOval(cx, cy, rx, ry, c, sparse = false) {
    g.fillStyle = c;
    for (let y = -ry; y <= ry; y++) {
      const w = half(rx, ry, y);
      for (let x = -w; x <= w; x++) {
        const X = cx + x, Y = cy + y;
        if (sparse ? (X & 1) === 0 && (Y & 1) === 0 : ((X + Y) & 1) === 0) g.fillRect(X, Y, 1, 1);
      }
    }
  }
  // A thick line (an arm, an antenna), as squares along the way.
  function stroke(x0, y0, x1, y1, size, c) {
    const n = Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0), 1);
    g.fillStyle = c;
    for (let i = 0; i <= n; i++) {
      const x = Math.round(x0 + ((x1 - x0) * i) / n), y = Math.round(y0 + ((y1 - y0) * i) / n);
      g.fillRect(x - (size >> 1), y - (size >> 1), size, size);
    }
  }
  // A small picture from rows of text: "#" is a pixel. `fade` keeps every other one.
  function sprite(rows, x, y, c, fade = false) {
    rows.forEach((row, j) => {
      for (let i = 0; i < row.length; i++) {
        if (row[i] === "#" && (!fade || ((x + i + y + j) & 1) === 0)) px(x + i, y + j, c);
      }
    });
  }
  // The same "random" number for the same n, so twinkles and meters don't need state.
  const hash = (n) => {
    const s = Math.sin(n * 127.1 + 311.7) * 43758.5453;
    return s - Math.floor(s);
  };

  const NOTES = [
    ["..##", "..#.#", "..#", "..#", "###", "##"],
    [".#####", ".#...#", ".#...#", "##..##", "##..##"],
  ];

  // --- Where things are ---------------------------------------------------------------------

  // The boombox: one big speaker, and the deck on the side where they sit.
  const BX = 24, BY = 70, BW = 62, BH = 30; // its body
  const KEYS = { rec: 56, pause: 62, play: 68, eject: 74 };
  const key = (name, down = 0) => ({ x: KEYS[name] + 2, y: 65 + down });
  const doorTop = (open) => 80 + Math.round(open * 8);
  const door = (open) => ({ x: 68, y: doorTop(open) });
  const SLOT = { x: 68, y: 83 }; // holding the tape where it goes into the deck
  const DECK_TAPE = { x: 68, y: 87 };
  // Where the wall meets the floor: some floor in front of it, so the room has depth.
  const FLOOR = 85;

  // Them, on a cushion beside it. Hands: the near one does everything; the far one rests.
  const SEAT = 108; // the middle of the cushion
  const SHOULDER = { x: SEAT - 11, y: 62 }, FAR_SHOULDER = { x: SEAT + 11, y: 62 };
  const KNEE = { x: 94, y: 83 }, FAR_KNEE = { x: 122, y: 83 };
  const LAP = { x: SEAT, y: 80 }, LOOK = { x: SEAT, y: 74 };
  const UPPER = 12, FORE = 12; // arm lengths: an arm doesn't stretch, they lean instead

  const ease = (p) => p * p * (3 - 2 * p);
  const mix = (a, b, p) => ({ x: Math.round(a.x + (b.x - a.x) * ease(p)), y: Math.round(a.y + (b.y - a.y) * ease(p)) });

  // --- The story, as a timeline. Each part moves the hand, the door or the tape. ------------

  // [seconds, what happens], in order; `done` is true for parts already over.
  const PARTS = [
    [1.6, () => {}],
    [0.4, (s, p) => { s.hand = mix(KNEE, key("eject"), p); }],
    [0.2, (s, p, done) => { s.hand = key("eject", 2); if (!done) s.down = "eject"; }],
    [0.45, (s, p) => { s.door = p; s.hand = mix(key("eject"), LAP, p); }],
    [0.25, (s) => { s.tape = "hand"; }],
    [0.7, (s, p) => { s.hand = mix(LAP, SLOT, p); }],
    [0.3, (s, p) => { s.tape = "deck"; s.hand = mix(SLOT, door(1), p); }],
    [0.35, (s, p) => { s.door = 1 - p; s.hand = door(1 - p); }],
    [0.35, (s, p) => { s.hand = mix(door(0), key("play"), p); }],
    [0.2, (s, p, done) => { s.hand = key("play", 2); if (!done) s.down = "play"; }],
    [0.4, (s, p) => { s.playing = true; s.hand = mix(key("play", 2), KNEE, p); }],
    [6.5, (s, p, done) => { if (!done) s.listen = p; }],
    [0.4, (s, p) => { s.hand = mix(KNEE, key("eject"), p); }],
    [0.2, (s, p, done) => { s.playing = false; s.hand = key("eject", 2); if (!done) s.down = "eject"; }],
    [0.45, (s, p) => { s.door = p; s.hand = key("eject"); }],
    [0.4, (s, p) => { s.hand = mix(key("eject"), SLOT, p); }],
    [0.7, (s, p) => { s.tape = "hand"; s.hand = mix(SLOT, LOOK, p); }],
    // Over to the other side: the tape narrows to its edge and comes back.
    [0.7, (s, p, done) => { if (!done) s.flip = p; s.turned = p >= 0.5; }],
    [0.3, (s, p) => { s.hand = mix(LOOK, LAP, p); }],
    [0.2, (s) => { s.tape = "lap"; }],
    [0.45, (s, p) => { s.hand = mix(LAP, door(1), p); }],
    [0.35, (s, p) => { s.door = 1 - p; s.hand = door(1 - p); }],
    [0.45, (s, p) => { s.hand = mix(door(0), KNEE, p); }],
  ];
  const LOOP = PARTS.reduce((n, [d]) => n + d, 0);
  // The still picture: a moment into the song.
  const STILL = PARTS.slice(0, 11).reduce((n, [d]) => n + d, 0) + 1.4;

  function stateAt(t) {
    const s = { hand: KNEE, tape: "lap", door: 0, down: null, playing: false, listen: -1, flip: 0, turned: false };
    let at = t % LOOP;
    for (const [d, part] of PARTS) {
      if (at >= d) {
        part(s, 1, true);
        at -= d;
      } else {
        part(s, at / d, false);
        break;
      }
    }
    // Side A, then B after each turn.
    s.side = (Math.floor(t / LOOP) + (s.turned ? 1 : 0)) % 2;
    return s;
  }

  // An arm of two parts that keep their lengths: the elbow bends down and out to its side
  // (`side`: -1 the near arm, 1 the far one), as an arm at rest does.
  function reach(from, to, side) {
    const dx = to.x - from.x, dy = to.y - from.y, max = UPPER + FORE - 0.5;
    let d = Math.hypot(dx, dy);
    const hand = d > max ? { x: from.x + (dx * max) / d, y: from.y + (dy * max) / d } : to;
    d = Math.min(Math.max(d, 1), max);
    const a = Math.atan2(dy, dx), bend = Math.acos(Math.max(-1, Math.min(1, (UPPER ** 2 + d ** 2 - FORE ** 2) / (2 * UPPER * d))));
    const e1 = { x: from.x + UPPER * Math.cos(a + bend), y: from.y + UPPER * Math.sin(a + bend) };
    const e2 = { x: from.x + UPPER * Math.cos(a - bend), y: from.y + UPPER * Math.sin(a - bend) };
    const lower = (e) => e.y + 0.7 * side * e.x;
    const elbow = lower(e1) > lower(e2) ? e1 : e2;
    const round = (p) => ({ x: Math.round(p.x), y: Math.round(p.y) });
    return { elbow: round(elbow), hand: round(hand) };
  }

  // --- The room ---------------------------------------------------------------------------

  function room(t) {
    rect(0, 0, W, FLOOR, P.wall);
    for (let x = 3; x < W; x += 8) rect(x, 0, 1, FLOOR, P.stripe);
    // Lamplight (or sunlight) where they sit, fading out.
    ditherOval(72, 58, 70, 42, P.glow, true);
    ditherOval(72, 58, 48, 30, P.glow);
    rect(0, FLOOR - 3, W, 3, P.base);
    rect(0, FLOOR, W, H - FLOOR, P.floor);
    // Floorboards, wider apart the nearer they are.
    let top = FLOOR;
    [89, 93, 98, 104, 111, 119].forEach((y, k) => {
      rect(0, y, W, 1, P.floorLine);
      for (let x = (k * 23) % 40; x < W; x += 40) rect(x, top, 1, y - top, P.floorLine);
      top = y + 1;
    });
    oval(70, 104, 76, 11, P.rugEdge);
    oval(70, 104, 73, 9, P.rug);
    oval(70, 104, 64, 6, P.rugEdge);
    oval(70, 104, 62, 5, P.rug);
    lights(t);
    windowView(t);
    shelf();
  }

  // Where something stands on the floor: a little shadow under it.
  function shadow(cx, cy, rx) {
    ditherOval(cx, cy, rx, 1, "rgba(43, 29, 34, 0.45)");
  }

  // Fairy lights along the top of the wall, a few dimming now and then.
  function lights(t) {
    const sag = (x) => 4 + Math.round(3 * Math.sin((Math.PI * (x % 40)) / 40));
    for (let x = 0; x < W; x++) px(x, sag(x), P.wire);
    for (let k = 0; k < 20; k++) {
      const x = 4 + 8 * k, y = sag(x) + 1;
      const on = hash(k * 7 + Math.floor(t * 1.3)) > 0.22;
      const c = on ? C.bulbs[k % 4] : P.bulbOff;
      if (on && P === NIGHT) ditherOval(x, y + 1, 2, 2, c);
      rect(x, y, 2, 2, c);
    }
  }

  function windowView(t) {
    const x = 6, y = 10, w = 48, h = 34;
    rect(x - 7, y - 4, w + 14, 1, C.out); // the curtain rod
    rect(x - 1, y - 1, w + 2, h + 2, C.out);
    rect(x, y, w, h, P.wood);
    g.save();
    g.beginPath();
    g.rect(x + 2, y + 2, w - 4, h - 4);
    g.clip();
    P.sky.forEach((c, i) => rect(x + 2, y + 2 + i * 10, w - 4, 10, c));
    if (P === NIGHT) {
      oval(x + 35, y + 9, 4, 4, P.moon);
      oval(x + 37, y + 8, 3, 3, P.sky[0]);
      [[5, 5], [13, 10], [20, 4], [8, 17], [41, 20], [17, 24], [30, 15], [27, 27], [6, 26], [44, 5]].forEach(([sx, sy], i) => {
        px(x + sx, y + sy, hash(i + Math.floor(t * 1.5) * 13) > 0.3 ? P.star : P.starDim);
      });
    } else {
      oval(x + 14, y + h - 6, 7, 7, P.sun);
      const cx = x + 2 + ((t * 1.5) % (w + 16)) - 12;
      oval(Math.round(cx), y + 9, 5, 2, P.cloud);
      oval(Math.round(cx) + 4, y + 7, 3, 2, P.cloud);
    }
    g.restore();
    rect(x + w / 2 - 1, y + 2, 2, h - 4, P.wood);
    rect(x + 2, y + h / 2 - 1, w - 4, 2, P.wood);
    rect(x - 3, y + h, w + 6, 3, P.wood);
    rect(x - 3, y + h + 3, w + 6, 1, C.out);
    // A small cactus on the sill.
    rect(x + 41, y + h - 4, 5, 4, P.pot);
    rect(x + 42, y + h - 9, 3, 5, P.plant);
    px(x + 41, y + h - 7, P.plant);
    px(x + 45, y + h - 8, P.plant);
    cat(t);
    // Curtains, tied back.
    for (const cx of [x - 5, x + w - 1]) {
      rect(cx, y - 3, 6, h + 9, P.curtain);
      rect(cx + 2, y - 3, 1, h + 9, P.curtainShade);
      rect(cx + 4, y - 3, 1, h + 9, P.curtainShade);
      rect(cx, y + 20, 6, 2, P.curtainShade);
    }
  }

  // A ginger cat asleep on the sill; it breathes, and its tail twitches over the edge.
  function cat(t) {
    const breath = Math.sin(t * 2) > 0 ? 1 : 0;
    const flick = Math.floor(t / 1.6) % 2;
    stroke(36, 44, 37, 51, 2, C.catDark);
    rect(37 + flick, 52, 2, 2, C.catDark);
    oval(29, 40, 10, 4 + breath, C.out);
    oval(29, 40, 9, 3 + breath, C.cat);
    [27, 31, 35].forEach((x) => rect(x, 37 - breath, 1, 2, C.catDark));
    const hx = 19, hy = 39;
    oval(hx, hy, 6, 5, C.out);
    oval(hx, hy, 5, 4, C.cat);
    for (const ex of [hx - 5, hx + 2]) {
      px(ex + 1, hy - 6, C.out);
      rect(ex, hy - 5, 3, 1, C.out);
      px(ex + 1, hy - 5, C.catPink);
    }
    rect(hx - 3, hy, 2, 1, C.out);
    rect(hx + 2, hy, 2, 1, C.out);
    px(hx, hy + 2, C.catPink);
  }

  // A shelf: books, a photo, and a plant trailing over the edge.
  function shelf() {
    const y = 42;
    rect(122, y, 36, 2, P.wood);
    rect(122, y + 2, 36, 1, C.out);
    rect(126, y + 3, 2, 3, P.wood);
    rect(152, y + 3, 2, 3, P.wood);
    [[124, 13], [127, 15], [130, 12], [133, 14]].forEach(([x, h], i) => {
      rect(x, y - h, 3, h, P.book[i]);
      rect(x, y - h + 2, 3, 1, C.label);
    });
    // One leaning on the others.
    stroke(137, y - 1, 140, y - 12, 2, P.book[0]);
    rect(142, y - 10, 9, 10, C.out);
    rect(143, y - 9, 7, 8, P.photo);
    sprite(["#.#", "###", ".#."], 145, y - 7, C.heart);
    rect(152, y - 6, 6, 6, P.pot);
    rect(152, y - 6, 6, 1, C.out);
    [[152, y - 8], [155, y - 9], [157, y - 7], [156, y - 1], [157, y + 3], [155, y + 7], [157, y + 10], [153, y + 13]].forEach(([x, yy], i) => {
      rect(x, yy, 2, 2, i % 2 ? P.plantDark : P.plant);
    });
  }

  // A mug of something warm, on the floor beside them.
  function mug(t) {
    shadow(137, 101, 6);
    rect(133, 92, 8, 9, C.out);
    rect(134, 93, 6, 7, P.mug);
    rect(134, 93, 6, 1, C.tape);
    rect(134, 96, 6, 1, P.mugStripe);
    rect(141, 94, 2, 4, C.out);
    px(141, 95, P.mug);
    px(141, 96, P.mug);
    for (let i = 0; i < 2; i++) {
      for (let k = 0; k < 3; k++) {
        const y = 90 - ((t * 7 + k * 4 + i * 6) % 12);
        const x = 135 + i * 3 + Math.round(Math.sin(y * 0.7 + i * 2));
        if ((Math.round(y) & 1) === 0 || y > 85) px(x, Math.round(y), P.steam);
      }
    }
  }

  // --- Them -----------------------------------------------------------------------------------

  // How far they lean towards the boombox, from how far the hand has to go: how far the top of
  // the back moves sideways (x) and the head down (y). They bend at the hips.
  function lean(hand) {
    const d = Math.hypot(hand.x - SHOULDER.x, hand.y - SHOULDER.y);
    const l = Math.max(0, Math.min(12, (d - 20) * 0.9));
    return { x: -l, y: Math.round(l * 0.35) };
  }
  // The back, from the neck (NECK) to the hips (HIPS): each row moves with the lean as much as
  // it's high above the hips.
  const NECK = 55, HIPS = 82;
  const bent = (shift, y) => Math.round((shift.x * (HIPS - y)) / (HIPS - NECK));

  function seat() {
    shadow(SEAT, 99, 25);
    oval(SEAT, 94, 24, 5, C.out);
    oval(SEAT, 94, 23, 4, C.cushion);
    rect(SEAT - 20, 96, 40, 1, C.cushionShade);
  }

  function person(s, t, shift) {
    const beat = s.playing && (t * 2.2) % 1 < 0.35;
    // A grey hoodie: shoulders, then straight down to the hips, row by row so it bends.
    for (let y = NECK - 1; y <= HIPS; y++) {
      const r = y - NECK, w = r < 0 ? 6 : [8, 10, 11, 12][r] ?? 13;
      const x = SEAT + bent(shift, y);
      rect(x - w - 1, y, 2 * w + 3, 1, C.out);
      if (r >= 0) rect(x - w, y, 2 * w + 1, 1, r === 1 ? C.hoodieLight : C.hoodie);
      if (y >= 73 && y <= 77) rect(x - 7, y, 14, 1, y === 73 ? C.hoodieLight : C.hoodieShade);
    }
    // Legs crossed, in front.
    oval(SEAT, 85, 20, 8, C.out);
    oval(SEAT, 85, 19, 7, C.pants);
    // The shin in front, crossing over the other.
    stroke(SEAT - 15, 83, SEAT + 15, 89, 1, C.pantsShade);
    rect(SEAT - 1, 79, 2, 3, C.pantsShade);
    for (const fx of [SEAT - 18, SEAT + 18]) {
      oval(fx, 89, 4, 3, C.out);
      oval(fx, 89, 3, 2, C.socks);
    }

    // The hood, bunched round the back of the neck like a thick collar, the chin resting on it.
    const nx = SEAT + bent(shift, NECK);
    oval(nx, NECK + 1, 10, 3, C.out);
    oval(nx, NECK + 1, 9, 2, C.hoodieShade);
    rect(nx - 3, NECK + 3, 1, 6, C.string);
    rect(nx + 3, NECK + 3, 1, 6, C.string);

    // The head goes a little further than the neck when they lean, and nods to the beat.
    const hx = SEAT + Math.round(shift.x * 1.1), hy = 43 + shift.y + (beat ? 1 : 0);
    rect(hx - 2, hy + 9, 4, Math.max(0, NECK + 1 - (hy + 9)), C.skinShade);
    oval(hx, hy, 12, 11, C.out);
    oval(hx, hy, 11, 10, C.hair);
    oval(hx, hy + 3, 9, 8, C.out);
    oval(hx, hy + 3, 8, 7, C.skin);
    // Short, a little untidy.
    rect(hx - 12, hy, 4, 7, C.out);
    rect(hx + 9, hy, 4, 7, C.out);
    rect(hx - 11, hy, 3, 6, C.hair);
    rect(hx + 9, hy, 3, 6, C.hair);
    for (let i = hx - 8; i <= hx + 8; i++) rect(i, hy - 5, 1, 3 + [0, 2, 1, 2, 0, 1][(i - hx + 8) % 6], C.hair);
    rect(hx - 6, hy - 7, 3, 1, C.hairLight);
    rect(hx + 2, hy - 8, 2, 1, C.hairLight);
    px(hx + 1, hy - 12, C.out);
    px(hx + 2, hy - 13, C.out);

    // Eyes on the boombox; closed for a while when the song gets good; down on the tape when
    // turning it over.
    const ey = hy + 3;
    if (s.listen > 0.45 && s.listen < 0.92) {
      for (const ex of [hx - 7, hx + 2]) {
        px(ex, ey + 1, C.out);
        px(ex + 1, ey, C.out);
        px(ex + 2, ey + 1, C.out);
      }
    } else if (t % 3.7 < 0.14) {
      rect(hx - 6, ey + 1, 2, 1, C.out);
      rect(hx + 3, ey + 1, 2, 1, C.out);
    } else {
      const down = s.flip > 0 ? 1 : 0;
      for (const ex of [hx - 6, hx + 3]) {
        rect(ex, ey + down, 2, 3 - down, C.out);
        px(ex, ey + down, C.white);
      }
    }
    px(hx - 7, hy + 6, C.blush);
    px(hx + 5, hy + 6, C.blush);
    if (s.listen > 0.45 && s.listen < 0.92) {
      rect(hx - 2, hy + 7, 2, 2, C.mouth);
    } else {
      rect(hx - 2, hy + 7, 2, 1, C.mouth);
    }
  }

  function arm(from, to, side) {
    const { elbow, hand } = reach(from, to, side);
    for (const [size, c] of [[7, C.out], [5, C.hoodie]]) {
      stroke(from.x, from.y, elbow.x, elbow.y, size, c);
      stroke(elbow.x, elbow.y, hand.x, hand.y, size, c);
    }
    // The cuff, then the hand.
    oval(hand.x, hand.y, 3, 3, C.hoodieShade);
    paw(hand);
    return hand;
  }
  function paw(hand) {
    oval(hand.x, hand.y, 3, 2, C.out);
    oval(hand.x, hand.y, 2, 1, C.skin);
  }

  // --- The boombox and the tape -------------------------------------------------------------

  // The tape, front on; `turn` (0–1) narrows it to its edge, as it's turned over.
  function tape(cx, cy, side, turn = 0) {
    const w = Math.max(2, Math.round(14 * Math.abs(Math.cos(Math.PI * turn))));
    const x = cx - (w >> 1), y = cy - 4;
    rect(x, y, w, 9, C.out);
    rect(x + 1, y + 1, w - 2, 7, C.shell);
    if (w < 8) return;
    rect(x + 2, y + 1, w - 4, 4, C.label);
    rect(x + 2, y + 2, w - 4, 1, C.sides[side]);
    rect(x + 4, y + 5, w - 8, 2, C.cavity);
    px(x + 5, y + 5, C.label);
    px(x + w - 6, y + 5, C.label);
    rect(x + 3, y + 7, w - 6, 1, C.shellDark);
  }

  // The tape lying on their lap.
  function tapeFlat(side) {
    rect(LAP.x - 7, LAP.y - 1, 14, 4, C.out);
    rect(LAP.x - 6, LAP.y, 12, 2, C.shell);
    rect(LAP.x - 4, LAP.y, 8, 1, C.label);
    rect(LAP.x - 2, LAP.y, 4, 1, C.sides[side]);
  }

  // A reel seen through the door's window: its spokes turn between + and ×.
  function reel(cx, cy, turn) {
    oval(cx, cy, 2, 2, C.reel);
    const spokes = turn ? [[-1, -1], [1, 1], [1, -1], [-1, 1]] : [[0, -1], [0, 1], [-1, 0], [1, 0]];
    for (const [dx, dy] of spokes) px(cx + dx, cy + dy, C.tape);
    px(cx, cy, C.cavity);
  }

  function boombox(s, t) {
    const beat = s.playing && (t * 2.2) % 1 < 0.35;
    shadow(BX + BW / 2, BY + BH + 2, BW / 2 + 2);
    // Handle and antenna.
    stroke(BX + 3, BY, BX - 1, BY - 14, 1, C.metal);
    oval(BX - 1, BY - 15, 1, 1, C.metal);
    rect(BX + 4, BY - 10, 57, 4, C.out);
    rect(BX + 4, BY - 10, 4, 11, C.out);
    rect(BX + 57, BY - 10, 4, 11, C.out);
    rect(BX + 5, BY - 9, 55, 2, C.handle);
    rect(BX + 5, BY - 9, 2, 9, C.handle);
    rect(BX + 58, BY - 9, 2, 9, C.handle);

    // The keys on top: record, pause, play (stays down while it plays), eject.
    for (const [name, x] of Object.entries(KEYS)) {
      const down = s.down === name || (name === "play" && s.playing) ? 2 : 0;
      const y = BY - 4 + down;
      rect(x, y, 5, 5, C.out);
      const c = { rec: C.red, pause: C.key, play: C.mint, eject: C.key }[name];
      rect(x + 1, y + 1, 3, 3, down ? C.keyDown : c);
      if (name === "pause") {
        px(x + 1, y + 2, C.out);
        px(x + 3, y + 2, C.out);
      }
      if (name === "eject") px(x + 2, y + 1, C.out);
    }

    // The body.
    rect(BX + 1, BY, BW - 2, BH, C.out);
    rect(BX, BY + 1, BW, BH - 2, C.out);
    rect(BX + 1, BY + 1, BW - 2, BH - 2, C.body);
    rect(BX + 2, BY + 1, BW - 4, 1, C.bodyLight);
    rect(BX + 1, BY + BH - 4, BW - 2, 3, C.bodyShade);
    rect(BX + 6, BY + BH, 6, 2, C.out);
    rect(BX + BW - 12, BY + BH, 6, 2, C.out);
    sprite(["..#..", ".###.", "#####", ".#.#."], BX + 2, BY + 2, C.star);

    // The speaker, pumping with the beat.
    const cx = BX + 14, cy = BY + 15;
    oval(cx, cy, 11, 11, C.out);
    oval(cx, cy, 10, 10, C.rim);
    oval(cx, cy, 8, 8, C.grill);
    ditherOval(cx, cy, 8, 8, C.grillDark);
    const r = beat ? 6 : 5;
    oval(cx, cy, r + 1, r + 1, C.out);
    oval(cx, cy, r, r, C.cone);
    oval(cx, cy, 2, 2, C.cap);
    px(cx - 1, cy - 1, C.white);

    // The level meter, then the deck and its door.
    rect(52, 72, 32, 5, C.out);
    rect(53, 73, 30, 3, C.lcd);
    for (let row = 0; row < 2; row++) {
      const level = s.playing ? 3 + Math.floor(hash(Math.floor(t * 9) * 3 + row) * 11) : 0;
      for (let i = 0; i < 14; i++) px(54 + i * 2, 73 + row * 2, i < level ? (i > 11 ? C.red : C.lcdOn) : C.lcdOff);
    }
    rect(52, 77, 32, 22, C.out);
    rect(53, 78, 30, 20, C.panel);
    rect(56, 79, 24, 17, C.out);
    rect(57, 80, 22, 15, C.cavity);
    if (s.door > 0) {
      if (s.tape === "deck") tape(DECK_TAPE.x, DECK_TAPE.y, s.side);
      const top = doorTop(s.door);
      rect(57, top, 22, 95 - top, C.doorInner);
      rect(57, top, 22, 1, C.doorLight);
      rect(57, top + 1, 22, 1, C.doorShade);
    } else {
      rect(57, 80, 22, 15, C.door);
      rect(57, 80, 22, 1, C.doorLight);
      rect(59, 82, 18, 8, C.out);
      rect(60, 83, 16, 6, C.cavity);
      if (s.tape === "deck") {
        rect(60, 83, 16, 1, C.sides[s.side]);
        const turn = s.playing && Math.floor(t * 8) % 2;
        reel(64, 86, turn);
        reel(72, 86, turn);
        rect(65, 88, 7, 1, C.tape);
      }
      rect(60, 92, 16, 1, C.doorShade);
    }
  }

  // Notes rising from the speaker while it plays, fading as they go.
  function notes(s, t) {
    if (!s.playing) return;
    for (let i = 0; i < 5; i++) {
      const life = (t * 0.5 + i / 5) % 1;
      const x = BX + 6 + (i % 2) * 12 + Math.round(Math.sin(life * 6 + i) * 3);
      const y = 66 - Math.round(life * 44);
      sprite(NOTES[i % 2], x, y, C.sides[i % 3 === 0 ? 1 : 0], life > 0.72);
    }
  }

  function draw(t) {
    const s = stateAt(t);
    const shift = lean(s.hand);
    const shoulder = { x: SHOULDER.x + bent(shift, SHOULDER.y), y: SHOULDER.y };
    const far = { x: FAR_SHOULDER.x + bent(shift, FAR_SHOULDER.y), y: FAR_SHOULDER.y };
    room(t);
    mug(t);
    seat();
    person(s, t, shift);
    if (s.tape === "lap") tapeFlat(s.side);
    // The far hand rests on a knee, tapping along while it plays.
    arm(far, { x: FAR_KNEE.x, y: FAR_KNEE.y - (s.playing && (t * 2.2) % 1 < 0.35 ? 1 : 0) }, 1);
    boombox(s, t);
    const hand = arm(shoulder, s.hand, -1);
    if (s.tape === "hand") {
      tape(hand.x, hand.y + 4, s.side, s.flip);
      paw(hand);
    }
    notes(s, t);
  }

  // --- Running it ---------------------------------------------------------------------------

  // As big as fits, in whole device pixels per pixel of the picture.
  function fit() {
    const dpr = devicePixelRatio || 1;
    const room = canvas.parentElement.clientWidth;
    const scale = Math.max(1, Math.floor((room * dpr) / W));
    canvas.style.width = `${(scale * W) / dpr}px`;
    canvas.style.height = `${(scale * H) / dpr}px`;
  }
  fit();
  if ("ResizeObserver" in window) new ResizeObserver(fit).observe(canvas.parentElement);

  const asked = Number(new URLSearchParams(location.search).get("room"));
  const frozen = Number.isFinite(asked) && asked > 0 ? asked : null;
  const start = performance.now();
  let visible = true, running = false, drawn = null;
  const now = () => frozen ?? (still.matches ? STILL : Math.floor(((performance.now() - start) / 1000) * 15) / 15);

  // The screen refreshes four times or more per frame of the picture: draw only when it changes.
  function frame() {
    const t = now();
    if (t !== drawn) {
      draw(t);
      drawn = t;
    }
    if (visible && !still.matches && frozen === null) requestAnimationFrame(frame);
    else running = false;
  }
  function go() {
    if (running) return;
    running = true;
    requestAnimationFrame(frame);
  }
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(([e]) => { visible = e.isIntersecting; if (visible) go(); }).observe(canvas);
  }
  dark.addEventListener("change", () => { P = dark.matches ? NIGHT : DAY; drawn = null; go(); });
  still.addEventListener("change", go);
  go();
})();
